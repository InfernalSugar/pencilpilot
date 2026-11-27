//! Opening a project without reading all of it, and saving it again without writing all
//! of it: for an app that holds the pixels itself (the native app's painter).
//!
//! [`Opened`] reads the manifest and checks it against the names of the file's entries,
//! leaving the entries (tiles, sounds, fonts, reference pictures, line data, the timelapse,
//! the stroke log, the thumbnail) in the file until they're asked for.
//! [`Project::write_from`] writes a project whose tiles and other entries needn't all be in
//! memory: whatever isn't is copied from the file it was opened from, as stored (no
//! decompressing and compressing again). Tiles are named by their content, so a tile kept
//! in the old file is the same tile in the new one.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, Write};

use super::{
    Error, MASK_TILE, Manifest, PALETTE_COLOURS_MAX, PALETTE_NAME_MAX, PALETTES_MAX, Project,
    RGBA_TILE, Result, STAGES, TIMELAPSE_MAX, VERSION, audio_name_ok, is_hex_colour, lines_name_ok,
};

/// The name a tile is stored under: its bytes' hash (as [`Project::put_drawing`] names
/// them).
pub fn tile_hash(bytes: &[u8]) -> String {
    super::hash(bytes)
}

/// A project file open to read from: the manifest (checked, as [`Project::read`] checks
/// it), and its entries, read when wanted.
pub struct Opened<R> {
    pub manifest: Manifest,
    zip: zip::ZipArchive<R>,
    /// entry name → its index in the archive
    names: std::sync::Arc<HashMap<String, usize>>,
    /// the timelapse pictures in order (the manifest's times): their entries
    timelapse: Vec<usize>,
}

/// A cheap reader of an [`Opened`] file's tiles, for reading them on many threads (each its
/// own [`Opened::tiles`], when `R` is cheap to clone: a `Cursor` over bytes in memory).
#[derive(Clone)]
pub struct Tiles<R> {
    zip: zip::ZipArchive<R>,
    names: std::sync::Arc<HashMap<String, usize>>,
}

impl<R: Read + Seek> Tiles<R> {
    /// A tile's bytes: RGBA (16 384) or a mask's grey (4 096).
    pub fn tile(&mut self, hash: &str) -> Result<Vec<u8>> {
        let i = *self
            .names
            .get(&format!("tiles/{hash}"))
            .ok_or_else(|| Error::Invalid(format!("missing tile {hash}")))?;
        let f = self.zip.by_index(i)?;
        let mut v = Vec::with_capacity(RGBA_TILE);
        f.take(RGBA_TILE as u64 + 1).read_to_end(&mut v)?;
        if v.len() != RGBA_TILE && v.len() != MASK_TILE {
            return Err(Error::Invalid(format!("tile {hash} has the wrong size")));
        }
        Ok(v)
    }
}

impl<R: Clone> Clone for Opened<R> {
    fn clone(&self) -> Self {
        Opened {
            manifest: self.manifest.clone(),
            zip: self.zip.clone(),
            names: self.names.clone(),
            timelapse: self.timelapse.clone(),
        }
    }
}

impl<R: Read + Seek> Opened<R> {
    /// The manifest, checked against the entries there are (by name; a tile's size is
    /// checked when it's read).
    pub fn read(r: R) -> Result<Self> {
        let mut zip = zip::ZipArchive::new(r)?;
        let mut manifest: Manifest = {
            let f = zip.by_name("manifest.json").map_err(|_| {
                Error::Invalid("not a PencilPilot project (no manifest.json)".into())
            })?;
            // (read whole first: serde reading a zip entry a byte at a time is slow)
            let mut json = Vec::new();
            f.take(64 << 20).read_to_end(&mut json)?;
            serde_json::from_slice(&json)?
        };
        if manifest.format == super::FORMAT && manifest.version > VERSION {
            return Err(Error::TooNew(manifest.version));
        }
        let mut names = HashMap::new();
        for i in 0..zip.len() {
            if let Some(Ok(n)) = zip.name_for_index(i) {
                names.insert(n.to_string(), i);
            }
        }
        // a stand-in project with every entry there is (empty), to check as `read` does
        let mut shell = Project::new("", "", 1, 1, 1);
        let mut pictures: BTreeMap<usize, usize> = BTreeMap::new();
        for (name, &i) in &names {
            if let Some(n) = name
                .strip_prefix("timelapse/")
                .and_then(|n| n.strip_suffix(".jpg"))
                .and_then(|n| n.parse::<usize>().ok())
            {
                if n < TIMELAPSE_MAX {
                    pictures.insert(n, i);
                }
            } else if let Some(h) = name.strip_prefix("audio/") {
                if !audio_name_ok(h) {
                    return Err(Error::Invalid(format!("sound file {h}")));
                }
                shell.audio.insert(h.into(), Vec::new());
            } else if let Some(h) = name.strip_prefix("fonts/") {
                if !audio_name_ok(h) {
                    return Err(Error::Invalid(format!("font file {h}")));
                }
                shell.fonts.insert(h.into(), Vec::new());
            } else if let Some(h) = name.strip_prefix("references/") {
                if !audio_name_ok(h) {
                    return Err(Error::Invalid(format!("reference picture {h}")));
                }
                shell.references.insert(h.into(), Vec::new());
            } else if let Some(h) = name.strip_prefix("lines/") {
                if !lines_name_ok(h) {
                    return Err(Error::Invalid(format!("line data {h}")));
                }
                shell.lines.insert(h.into(), Vec::new());
            } else if let Some(h) = name.strip_prefix("tiles/") {
                shell.tiles.insert(h.into(), Vec::new());
            }
        }
        // (as `read` does: pictures that don't line up with their times are dropped,
        // and stages and palettes it doesn't know)
        let mut timelapse = Vec::new();
        let mut times = Vec::new();
        for (i, t) in manifest.timelapse.iter().enumerate().take(TIMELAPSE_MAX) {
            match pictures.get(&i) {
                Some(&at) => {
                    timelapse.push(at);
                    times.push(*t);
                }
                None => break,
            }
        }
        manifest.timelapse = times;
        let layer_ids: Vec<u32> = manifest.layers.iter().map(|l| l.id).collect();
        manifest.timeline.layer_stages.retain(|layer, stage| {
            layer.parse::<u32>().is_ok_and(|id| layer_ids.contains(&id))
                && STAGES.contains(&stage.as_str())
        });
        let pals = &mut manifest.timeline.palettes;
        pals.truncate(PALETTES_MAX);
        for p in pals.iter_mut() {
            p.name = p.name.chars().take(PALETTE_NAME_MAX).collect();
            p.colours.retain(|c| is_hex_colour(&c.hex));
            p.colours.truncate(PALETTE_COLOURS_MAX);
            for c in &mut p.colours {
                c.name = c.name.chars().take(PALETTE_NAME_MAX).collect();
            }
        }
        shell.timelapse = vec![Vec::new(); timelapse.len()];
        shell.manifest = manifest;
        shell.validate()?;
        Ok(Opened {
            manifest: shell.manifest,
            zip,
            names: std::sync::Arc::new(names),
            timelapse,
        })
    }

    /// The same file through another reader (the file itself, after reading it from bytes
    /// in memory): the manifest and its checks not done again. (It must be the same file.)
    pub fn with_reader<R2: Read + Seek>(&self, r: R2) -> Result<Opened<R2>> {
        let zip = zip::ZipArchive::new(r)?;
        if zip.len() != self.zip.len() {
            return Err(Error::Invalid("the file changed".into()));
        }
        Ok(Opened {
            manifest: self.manifest.clone(),
            zip,
            names: self.names.clone(),
            timelapse: self.timelapse.clone(),
        })
    }

    /// A reader of the tiles alone (cheap to clone when `R` is).
    pub fn tiles(&self) -> Tiles<R>
    where
        R: Clone,
    {
        Tiles {
            zip: self.zip.clone(),
            names: self.names.clone(),
        }
    }

    /// Whether the file has this entry ("tiles/<hash>", "audio/<file>"…).
    pub fn has(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }

    /// Whether the file has this tile.
    pub fn has_tile(&self, hash: &str) -> bool {
        self.names.contains_key(&format!("tiles/{hash}"))
    }

    /// An entry's bytes (at most `limit`; more is an error).
    pub fn entry(&mut self, name: &str, limit: usize) -> Result<Vec<u8>> {
        let i = *self
            .names
            .get(name)
            .ok_or_else(|| Error::Invalid(format!("no {name}")))?;
        let f = self.zip.by_index(i)?;
        let mut v = Vec::new();
        f.take(limit as u64 + 1).read_to_end(&mut v)?;
        if v.len() > limit {
            return Err(Error::Invalid(format!("{name} is too big")));
        }
        Ok(v)
    }

    /// A tile's bytes: RGBA (16 384) or a mask's grey (4 096).
    pub fn tile(&mut self, hash: &str) -> Result<Vec<u8>> {
        let v = self.entry(&format!("tiles/{hash}"), RGBA_TILE)?;
        if v.len() != RGBA_TILE && v.len() != MASK_TILE {
            return Err(Error::Invalid(format!("tile {hash} has the wrong size")));
        }
        Ok(v)
    }

    /// Copy an entry as stored into a zip being written.
    fn copy<W: Write + Seek>(&mut self, name: &str, z: &mut zip::ZipWriter<W>) -> Result<()> {
        let i = *self
            .names
            .get(name)
            .ok_or_else(|| Error::Invalid(format!("missing {name}")))?;
        z.raw_copy_file(self.zip.by_index_raw(i)?)?;
        Ok(())
    }
}

impl Project {
    /// Write the project as [`Project::write`] does, taking what isn't in memory from
    /// `from` (the file it was opened from, or last saved to), copied as stored: tiles not
    /// in [`Project::tiles`], sounds, fonts, reference pictures and line data not in their
    /// maps, timelapse pictures past [`Project::timelapse`]'s, the stroke log when
    /// [`Project::strokes`] is empty, the thumbnail when there's none.
    pub fn write_from<W: Write + Seek, R: Read + Seek>(
        &self,
        w: W,
        mut from: Option<&mut Opened<R>>,
    ) -> Result<()> {
        use zip::write::SimpleFileOptions;
        let mut z = zip::ZipWriter::new(w);
        let deflate =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        // put an entry from memory, else copy it from `from`
        macro_rules! put {
            ($name:expr, $bytes:expr, $opts:expr) => {{
                let name: String = $name;
                match $bytes {
                    Some(b) => {
                        z.start_file(name, $opts)?;
                        z.write_all(b)?;
                    }
                    None => match from.as_deref_mut() {
                        Some(f) => f.copy(&name, &mut z)?,
                        None => return Err(Error::Invalid(format!("missing {name}"))),
                    },
                }
            }};
        }
        z.start_file("manifest.json", deflate)?;
        z.write_all(&serde_json::to_vec_pretty(&self.manifest)?)?;
        match from.as_deref_mut() {
            Some(f) if self.strokes.is_empty() && f.has("strokes.json") => {
                f.copy("strokes.json", &mut z)?
            }
            _ => {
                z.start_file("strokes.json", deflate)?;
                z.write_all(if self.strokes.is_empty() {
                    b"[]"
                } else {
                    &self.strokes
                })?;
            }
        }
        match (&self.thumbnail, from.as_deref_mut()) {
            (Some(t), _) => {
                z.start_file("thumbnail.png", stored)?;
                z.write_all(t)?;
            }
            (None, Some(f)) if f.has("thumbnail.png") => f.copy("thumbnail.png", &mut z)?,
            _ => {}
        }
        let mut used: Vec<&String> = self
            .manifest
            .drawings
            .values()
            .flat_map(|d| d.tiles.iter().chain(d.mask.iter().flatten()))
            .map(|r| &r.hash)
            .collect();
        used.sort();
        used.dedup();
        for h in used {
            put!(format!("tiles/{h}"), self.tiles.get(h), deflate);
        }
        let mut sounds: Vec<&String> = self
            .manifest
            .timeline
            .scenes
            .iter()
            .flat_map(|s| s.audio.iter().map(|a| &a.file))
            .collect();
        sounds.sort();
        sounds.dedup();
        for name in sounds {
            put!(format!("audio/{name}"), self.audio.get(name), stored);
        }
        let mut fonts: Vec<&String> = self.manifest.fonts.iter().map(|f| &f.file).collect();
        fonts.sort();
        fonts.dedup();
        for name in fonts {
            put!(format!("fonts/{name}"), self.fonts.get(name), stored);
        }
        let mut refs: Vec<&String> = self.manifest.references.iter().map(|r| &r.file).collect();
        refs.sort();
        refs.dedup();
        for name in refs {
            put!(
                format!("references/{name}"),
                self.references.get(name),
                stored
            );
        }
        // line data: what's in memory, and what the old file has for drawings still here
        let mut lines: Vec<&String> = self
            .manifest
            .drawings
            .keys()
            .filter(|name| {
                lines_name_ok(name)
                    && (self.lines.contains_key(*name)
                        || from
                            .as_deref()
                            .is_some_and(|f| f.has(&format!("lines/{name}"))))
            })
            .collect();
        lines.sort();
        for name in lines {
            put!(format!("lines/{name}"), self.lines.get(name), deflate);
        }
        for i in 0..self.manifest.timelapse.len() {
            match self.timelapse.get(i) {
                Some(jpeg) => {
                    z.start_file(format!("timelapse/{i:06}.jpg"), stored)?;
                    z.write_all(jpeg)?;
                }
                None => {
                    let f = from
                        .as_deref_mut()
                        .ok_or_else(|| Error::Invalid("the timelapse".into()))?;
                    let at = *f
                        .timelapse
                        .get(i)
                        .ok_or_else(|| Error::Invalid("the timelapse".into()))?;
                    // (renamed in order: the old file may have numbered them otherwise)
                    z.raw_copy_file_rename(
                        f.zip.by_index_raw(at)?,
                        format!("timelapse/{i:06}.jpg"),
                    )?;
                }
            }
        }
        z.finish()?;
        Ok(())
    }

    /// [`Project::save`]'s safe write (next to it, then moved into place), with
    /// [`Project::write_from`].
    pub fn save_from<R: Read + Seek>(
        &self,
        path: &std::path::Path,
        from: Option<&mut Opened<R>>,
    ) -> Result<()> {
        let tmp = path.with_extension("ppstudio.saving");
        let res = (|| {
            let mut w = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
            self.write_from(&mut w, from)?;
            w.into_inner()
                .map_err(|e| Error::Io(e.into_error()))?
                .sync_all()?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        })();
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cut, Exposure, Layer, Scene};
    use std::io::Cursor;

    fn project() -> Project {
        let (w, h) = (150u32, 100u32);
        let mut p = Project::new("test", "2026-11-27T00:00:00Z", w, h, 24);
        p.manifest.layers.push(Layer {
            id: 1,
            name: "Ink".into(),
            visible: true,
            opacity: 1.0,
            blend: "normal".into(),
            locked: false,
            alpha_lock: false,
            clip: false,
            imported: false,
            group: 0,
            adjust: None,
            style: None,
            text: None,
            lines: false,
        });
        for (k, name) in ["D1", "D2"].iter().enumerate() {
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for i in (0..rgba.len()).step_by(4 * (7 + k)) {
                rgba[i..i + 4].copy_from_slice(&[200, 10 * k as u8, 30, 255]);
            }
            p.put_drawing(name, &rgba, None).unwrap();
        }
        p.lines.insert("D1".into(), b"lines!".to_vec());
        p.strokes = b"[1,2,3]".to_vec();
        p.thumbnail = Some(b"png".to_vec());
        let name = p.put_audio(b"RIFF....", "wav").unwrap();
        p.manifest.timeline.scenes = vec![Scene {
            name: "S".into(),
            cuts: vec![Cut {
                name: "C".into(),
                tracks: [(
                    1,
                    vec![
                        Exposure {
                            drawing: Some("D1".into()),
                            length: 2,
                        },
                        Exposure {
                            drawing: Some("D2".into()),
                            length: 1,
                        },
                    ],
                )]
                .into(),
                ..Default::default()
            }],
            audio: vec![crate::AudioClip {
                name: "beat".into(),
                file: name,
                start: 0,
                gain: 1.0,
                muted: false,
            }],
            markers: vec![],
        }];
        p.timelapse = vec![b"jpeg0".to_vec(), b"jpeg1".to_vec()];
        p.manifest.timelapse = vec![1, 2];
        p
    }

    /// Opened lazily, changed a little and written from the old file: the same as the
    /// whole project written (everything not in memory copied over).
    #[test]
    fn saving_from_the_old_file_keeps_everything() {
        let p = project();
        let mut buf = Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        let bytes = buf.into_inner();
        let mut o = Opened::read(Cursor::new(bytes.clone())).unwrap();
        assert_eq!(
            o.manifest,
            Project::read(Cursor::new(bytes)).unwrap().manifest
        );
        let first = o.manifest.drawings["D1"].tiles[0].hash.clone();
        assert!(o.has_tile(&first));
        assert_eq!(o.tile(&first).unwrap(), p.tiles[&first]);
        // a project with nothing in memory but D2 drawn again
        let mut q = Project::new("test", "now", 150, 100, 24);
        q.manifest = o.manifest.clone();
        q.strokes.clear();
        let rgba = vec![9u8; 150 * 100 * 4];
        q.put_drawing("D2", &rgba, None).unwrap();
        let mut out = Cursor::new(Vec::new());
        q.write_from(&mut out, Some(&mut o)).unwrap();
        let back = Project::read(Cursor::new(out.into_inner())).unwrap();
        assert_eq!(back.drawing("D1").unwrap(), p.drawing("D1").unwrap());
        assert_eq!(back.drawing("D2").unwrap().0, rgba);
        assert_eq!(back.lines, p.lines);
        assert_eq!(back.strokes, p.strokes);
        assert_eq!(back.thumbnail, p.thumbnail);
        assert_eq!(back.audio, p.audio);
        assert_eq!(back.timelapse, p.timelapse);
        // with nothing to copy from, what's missing is an error, not a broken file
        let mut q2 = q.clone();
        q2.tiles.clear();
        assert!(
            q2.write_from::<_, Cursor<Vec<u8>>>(Cursor::new(Vec::new()), None)
                .is_err()
        );
    }

    #[test]
    fn opened_checks_what_read_checks() {
        let mut p = project();
        p.manifest.timeline.scenes[0].cuts[0]
            .tracks
            .get_mut(&1)
            .unwrap()[0]
            .drawing = Some("D9".into());
        let mut buf = Cursor::new(Vec::new());
        p.write(&mut buf).unwrap();
        assert!(Opened::read(Cursor::new(buf.into_inner())).is_err());
    }
}
