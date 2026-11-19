# ADR-0003 — Reference images: licence-clean, SFW, human-approved

**Status:** Accepted (2026-11-18). Sources updated 2026-11-19: Openverse, the Art Institute of Chicago and The Met added; Unsplash dropped; franchise block applies to every source.

## Allowed sources
| Source | Licence | Used for | Credit shown |
|---|---|---|---|
| Pixabay (API) | Pixabay Content License | Every category except Master studies | "By X via Pixabay" + licence link; cache API results 24 h; no mass downloading |
| Openverse (API) | **CC0, Public Domain Mark or CC BY only** | Real life, animals, environment (photos, mostly Wikimedia Commons) and Master studies (museums only) | Openverse's own attribution text, the provider, the licence link |
| Art Institute of Chicago (API) | Public domain (`is_public_domain`; images CC0) | Master studies | "Artwork: Title, Artist, Art Institute of Chicago (public domain)" |
| The Met (Open Access API) | Public domain (`isPublicDomain`; CC0) | Master studies | "Artwork: Title, Artist, The Metropolitan Museum of Art (public domain)" |
| Pexels | Pexels License | Not used yet: Pexels paused new API keys (2026-11). Can be added for photo categories when key issuance reopens ("Photo by X on Pexels") | — |
| OpenGameArt | **CC0 or CC-BY only** | Hand-approved manifests only | Keep attribution |
| rawpixel public domain | Public domain | Hand-approved manifests only | — |

**Not used:** Unsplash (user decision), and any NC, ND or SA licence from any source.

### Openverse rules
- **Licences:** the search asks for `license=cc0,pdm,by` and `license_type=commercial,modification`, and every result's own `license` field is checked again. Anything else (BY-NC, BY-ND, BY-SA, BY-NC-SA, BY-NC-ND, sampling, unknown) is dropped.
- **Providers (allow-list):** photos: `wikimedia`, `nasa`, `katmai_nps`, `smithsonian_zoo_and_conservation`, `smithsonian_gardens`. Art (Master studies): `met`, `clevelandmuseum`, `rijksmuseum`, `smk`, `brooklynmuseum`, `nypl`, `wellcome_collection`, `bio_diversity`, `smithsonian_american_art_museum`, `smithsonian_portrait_gallery`, `smithsonian_freer_gallery_of_art`, `smithsonian_cooper_hewitt_museum`. Wikimedia Commons is allowed for photos because its volunteers check licences and delete copyright violations. It is not used for Master studies: a photo of a painting carries the photographer's licence, not the painter's (a 1960s painting is still under copyright).
- **Never:** Flickr, iNaturalist, Geograph, WordPress Photo Directory, StockSnap, Rawpixel-via-Openverse, Sketchfab, Thingiverse and other sites where anyone can upload anything under any licence (licence laundering is common).
- No Openverse pictures for real-life People and Hands: Wikimedia photos of real people have no model release (personality rights). Pixabay covers those topics. A source only ever fills topics that list queries for it, even when the picture sorter would re-file a picture elsewhere.
- Results flagged `mature` or with any `unstable__sensitivity` are dropped.
- Rate limits: one call every 4 s, at most 150 a day anonymously (Openverse allows 200); `OPENVERSE_TOKEN` raises it. A 429 or a "0 left" header pauses the source.

### Museum rules
- Only works the museum marks public domain (checked on every record, never assumed from the search).
- Only flat works a student can copy: paintings, drawings, watercolours, prints. No sculpture, photographs or objects.

## Banned
Pinterest, Danbooru/Safebooru, Google Images, ArtStation packs, and anything depicting existing franchise characters, logos or brands (fan art). Also **AI-generated images**: they often have broken anatomy, which would teach mistakes.

**Franchise / fan-art block (every source, Pixabay included; user decision 2026-11-19):**
1. **Words.** A picture is dropped if its title or tags name a franchise, studio, character or brand, or say it is fan art or cosplay (Naruto, Gaara, Doraemon, Pokémon, Disney, Marvel, Star Wars, Harry Potter, Ghibli, LEGO, Nike, …). The list is `FRANCHISE` in `crates/ref-filters/src/words.rs`; whole words only, so "spider", "frozen lake" or "a sailor" still pass. Used by ref-service (every source), `ref-importer fetch`, and on the existing pool at every start.
2. **Picture.** For anime and cartoon, the picture sorter (ADR-0009) also compares each picture with ~40 well-known franchise characters and ~25 look-alike originals ("an anime girl with long blue hair"). It drops the picture if a franchise character beats both the topic and every look-alike by 0.5 (`_sorting.franchise` in `content/topics.json`). Photo categories rely on the word list (cosplay, brands), because the model confused ordinary photos of women and cats with Disney princesses and Hello Kitty.


## Process
1. `ref-importer` pulls candidates with SafeSearch on and records source URL, author, licence and licence URL.
2. Each image goes into an approval queue. A person checks it for NSFW content, AI artefacts, franchise characters and quality before setting `approved: true`.
3. Keypoints are marked in `ref-annotator`.
4. CI fails if any manifest entry lacks source, author, licence or approval.
5. Image files are never committed. The site fetches and caches them, and always shows attribution.
6. Endless (live) pictures from every source go through the same automatic checks (ADR-0005, ADR-0009): source licence and provider rules, banned words and franchise names, NSFW guard, picture sorter (category, topic, one subject, photos-only rule), near-duplicate check across all sources. Each stores source, author, author URL, page URL, licence, licence URL, provider and (Openverse) the attribution text.
