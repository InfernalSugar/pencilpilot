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

/// Franchises, studios, brands and their characters (ADR-0003: no fan art,
/// no logos or brands). Drawing them is fine for a student, but a site that
/// hands them out as references would be redistributing someone else's
/// characters. Only names that are unambiguous on their own; first names
/// that are also common words or names (Elsa, Mario, Kirby, Link, Frozen)
/// are left to the picture sorter's franchise check.
pub const FRANCHISE: &[&str] = &[
    // fan art in general
    "fanart",
    "fan art",
    "fan made",
    "fanmade",
    "fan fiction",
    "cosplay",
    "cosplayer",
    // anime and manga
    "naruto",
    "sasuke",
    "kakashi",
    "gaara",
    "hinata hyuga",
    "itachi",
    "boruto",
    "akatsuki",
    "dragon ball",
    "dragonball",
    "goku",
    "vegeta",
    "one piece",
    "luffy",
    "zoro",
    "sailor moon",
    "demon slayer",
    "kimetsu",
    "tanjiro",
    "nezuko",
    "attack on titan",
    "shingeki",
    "my hero academia",
    "boku no hero",
    "jujutsu kaisen",
    "gojo",
    "chainsaw man",
    "inosuke",
    "deku",
    "anya forger",
    "krillin",
    "spy x family",
    "death note",
    "evangelion",
    "bleach anime",
    "hunter x hunter",
    "fullmetal alchemist",
    "tokyo ghoul",
    "sword art online",
    "fairy tail",
    "inuyasha",
    "card captor",
    "cardcaptor",
    "doraemon",
    "nobita",
    "totoro",
    "ghibli",
    "spirited away",
    "howl s moving castle",
    "kiki s delivery",
    "hatsune miku",
    "vocaloid",
    "genshin",
    "honkai",
    "hololive",
    "pokemon",
    "pikachu",
    "eevee",
    "charizard",
    "digimon",
    "yu gi oh",
    "yugioh",
    "hello kitty",
    "sanrio",
    "kuromi",
    "cinnamoroll",
    "pompompurin",
    "my melody",
    "rilakkuma",
    // western animation, film, comics
    "disney",
    "pixar",
    "dreamworks",
    "mickey",
    "minnie mouse",
    "donald duck",
    "winnie the pooh",
    "stitch disney",
    "lilo and stitch",
    "toy story",
    "buzz lightyear",
    "frozen disney",
    "moana",
    "simba",
    "lion king",
    "shrek",
    "minions",
    "despicable me",
    "spongebob",
    "simpsons",
    "peppa pig",
    "paw patrol",
    "bluey",
    "scooby doo",
    "looney tunes",
    "bugs bunny",
    "tom and jerry",
    "smurf",
    "smurfs",
    "snoopy",
    "peanuts comic",
    "garfield cat",
    "my little pony",
    "teletubbies",
    "transformers",
    "marvel",
    "avengers",
    "spiderman",
    "spider man",
    "iron man",
    "captain america",
    "deadpool",
    "wolverine",
    "x men",
    "dc comics",
    "batman",
    "superman",
    "wonder woman",
    "joker batman",
    "harley quinn",
    "star wars",
    "jedi",
    "sith",
    "darth vader",
    "stormtrooper",
    "yoda",
    "grogu",
    "mandalorian",
    "lightsaber",
    "harry potter",
    "hogwarts",
    "lord of the rings",
    "hobbit",
    "star trek",
    "godzilla",
    "barbie",
    // games
    "super mario",
    "nintendo",
    "zelda",
    "sonic the hedgehog",
    "minecraft",
    "fortnite",
    "free fire",
    "roblox",
    "among us",
    "pac man",
    "pacman",
    "tetris",
    "angry birds",
    "overwatch",
    "league of legends",
    "playstation",
    "xbox",
    // brands and logos
    "lego",
    "nike",
    "adidas",
    "coca cola",
    "pepsi",
    "mcdonald",
    "mcdonalds",
    "starbucks",
    "harley davidson",
    "ferrari",
    "lamborghini",
];

/// Why `text` (a title, tags, a description) rules a picture out, if it
/// does: `unsafe_word:<word>` or `franchise:<name>`.
pub fn banned(text: &str) -> Option<String> {
    find(UNSAFE, text)
        .map(|w| format!("unsafe_word:{w}"))
        .or_else(|| find(FRANCHISE, text).map(|w| format!("franchise:{w}")))
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

    #[test]
    fn franchise_names() {
        for (text, name) in [
            ("naruto, anime, ninja", "naruto"),
            ("Gaara of the sand", "gaara"),
            ("doraemon, cartoon", "doraemon"),
            ("Pokémon trainer", "pokemon"),
            ("pikachu", "pikachu"),
            ("Mickey Mouse", "mickey"),
            ("marvel, superhero", "marvel"),
            ("Spider-Man swinging", "spider man"),
            ("fan art, anime girl", "fan art"),
            ("fanart", "fanart"),
            ("cosplay, girl", "cosplay"),
            ("star wars, lightsaber", "star wars"),
            ("Studio Ghibli style", "ghibli"),
            ("Howl's Moving Castle", "howl s moving castle"),
        ] {
            assert_eq!(banned(text), Some(format!("franchise:{name}")), "{text}");
        }
        // ordinary words that only look like names
        for ok in [
            "frozen lake",
            "spider on a web",
            "a sailor on a boat",
            "superhero kid in a cape",
            "Mario Rossi, portrait",
            "iron gate",
            "wonderful woman",
            "jungle, leaves",
            "apple, fruit",
        ] {
            assert_eq!(banned(ok), None, "{ok}");
        }
    }
}
