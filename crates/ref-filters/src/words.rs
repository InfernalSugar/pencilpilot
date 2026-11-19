//! Words that rule a reference picture out, matched against its title, tags
//! and description (whole words, any case, accents ignored).
//!
//! This is the cheap first check. The NSFW guard and the picture sorter look
//! at the picture itself afterwards.

/// AI-made, unsafe for children, or violent.
pub const UNSAFE: &[&str] = &[
    // AI-made (ADR-0003: broken anatomy teaches mistakes)
    "ai generated",
    "ai art",
    "ai image",
    "generative ai",
    "midjourney",
    "stable diffusion",
    "dall e",
    // nudity and adult content (classical art too: the NSFW guard is
    // trained on photos and can miss painted nudes)
    "nude",
    "nudes",
    "naked",
    "nudity",
    "topless",
    "sexy",
    "erotic",
    "erotica",
    "lingerie",
    "bikini",
    "underwear",
    "odalisque",
    "bather",
    "bathers",
    "leda",
    "danae",
    // violence and hate
    "gore",
    "blood",
    "bloody",
    "corpse",
    "gun",
    "guns",
    "rifle",
    "pistol",
    "revolver",
    "torture",
    "execution",
    "beheading",
    "decapitation",
    "lynching",
    "swastika",
    "nazi",
];

/// Lower case, accents dropped, everything but letters and digits turned
/// into single spaces, padded with a space on each side.
pub fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(' ');
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c => c,
        };
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// The first word or phrase of `list` that occurs in `text` as whole words.
pub fn find<'a>(list: &[&'a str], text: &str) -> Option<&'a str> {
    let t = normalise(text);
    list.iter().copied().find(|w| t.contains(&normalise(w)))
}

/// Why `text` (a title, tags, a description) rules a picture out, if it
/// does: `unsafe_word:<word>`.
pub fn banned(text: &str) -> Option<String> {
    find(UNSAFE, text).map(|w| format!("unsafe_word:{w}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_words_only() {
        assert_eq!(normalise("Hello, World!"), " hello world ");
        assert_eq!(find(&["gun"], "a burgundy shotgun"), None);
        assert_eq!(find(&["gun"], "toy gun, kids"), Some("gun"));
        assert_eq!(find(&["dall e"], "made with DALL-E 3"), Some("dall e"));
        assert_eq!(
            find(&["ai generated"], "AI-generated portrait"),
            Some("ai generated")
        );
        assert_eq!(find(&["pokemon"], "Pokémon card"), Some("pokemon"));
        assert_eq!(find(&["nude"], "Nude Descending"), Some("nude"));
        assert_eq!(find(&["nude"], "nudes"), None);
    }

    #[test]
    fn banned_words() {
        assert_eq!(
            banned("Reclining Nude, oil on canvas").as_deref(),
            Some("unsafe_word:nude")
        );
        assert_eq!(banned("Still Life with Apples"), None);
        assert_eq!(
            banned("Bathers at Asnières").as_deref(),
            Some("unsafe_word:bathers")
        );
        assert_eq!(banned("blood orange"), Some("unsafe_word:blood".into()));
    }
}
