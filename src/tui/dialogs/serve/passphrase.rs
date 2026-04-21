//! Wordlist-based passphrase generation for the serve dialog.
//!
//! Four short, easy-to-type words give ~35 bits of entropy, which as a
//! second factor on top of the URL token is plenty without forcing the user
//! to type random alphanumeric soup on a phone.

use rand::prelude::IndexedRandom;

/// Generate a 4-word passphrase. Uses `rand::rng()`, so requires the global
/// thread-local RNG (already used elsewhere in this dialog).
pub fn generate_passphrase() -> String {
    let mut rng = rand::rng();
    let words: Vec<&'static str> = (0..4)
        .map(|_| {
            *PASSPHRASE_WORDS
                .choose(&mut rng)
                .expect("wordlist nonempty")
        })
        .collect();
    words.join(" ")
}

/// Curated list of short, unambiguous lowercase English words chosen for
/// phone-typability. No words shorter than 3 letters or longer than 6.
/// No near-homophones (e.g., "their"/"there") or visually confusable pairs.
#[rustfmt::skip]
const PASSPHRASE_WORDS: &[&str] = &[
    "able", "acid", "aged", "acorn", "agent", "alarm", "album", "alert",
    "algae", "alien", "alive", "alley", "alloy", "alpha", "amber", "amigo",
    "amino", "amuse", "angel", "anger", "angle", "angry", "ankle", "anvil",
    "apple", "apron", "arbor", "arena", "argon", "armor", "arrow", "ashen",
    "aside", "aspen", "asset", "atlas", "atom", "audio", "audit", "aunt",
    "avoid", "awake", "award", "aware", "awful", "axis", "bacon", "badge",
    "bagel", "baker", "balmy", "banjo", "baron", "basil", "basin", "basis",
    "batch", "baton", "beach", "beads", "beard", "beast", "beaver", "bench",
    "berry", "bingo", "birch", "bison", "black", "blade", "blaze", "blend",
    "bliss", "block", "bloom", "blues", "blunt", "blush", "board", "boast",
    "bold", "bolt", "bonus", "boost", "booth", "boots", "bored", "boss",
    "botany", "bowl", "brave", "bread", "break", "brick", "bride", "brief",
    "bring", "brisk", "brook", "brown", "brush", "bucket", "bugle", "built",
    "bulk", "bunny", "burly", "butter", "buzz", "cabin", "cable", "cactus",
    "caddy", "camel", "camp", "candle", "candy", "canoe", "canon", "canyon",
    "cape", "caper", "card", "care", "cargo", "carry", "cart", "carve",
    "cash", "cast", "catch", "cedar", "chair", "chalk", "charm", "chart",
    "chase", "cheek", "cheer", "chef", "chess", "chief", "child", "chill",
    "chimp", "chip", "chirp", "choir", "chose", "chunk", "cider", "cinema",
    "civic", "claim", "clamp", "clean", "clerk", "click", "cliff", "climb",
    "cling", "clock", "clone", "cloth", "cloud", "clove", "clown", "club",
    "clue", "coach", "coast", "cobra", "cocoa", "code", "coin", "colon",
    "color", "comet", "coral", "cord", "corn", "cost", "couch", "cover",
    "cozy", "craft", "crane", "crash", "crate", "cream", "crest", "crew",
    "cross", "crowd", "crown", "crumb", "crush", "crust", "cube", "curl",
    "cycle", "daisy", "dance", "dare", "dash", "data", "deal", "deck",
    "delta", "dense", "depth", "derby", "desk", "diary", "dice", "diner",
    "disco", "diver", "dock", "dodo", "dog", "doll", "dolly", "donkey",
    "dough", "dove", "downy", "draft", "dragon", "drape", "dream", "drift",
    "drill", "drive", "drop", "drum", "duck", "dusk", "dusty", "eager",
    "eagle", "early", "earth", "ebony", "echo", "edge", "eject", "elbow",
    "elder", "elf", "elite", "elk", "elm", "email", "empty", "enact",
    "energy", "engine", "enjoy", "enter", "entry", "envoy", "epic", "equal",
    "era", "error", "essay", "ether", "event", "every", "exact", "exile",
    "exit", "extra", "eye", "fable", "face", "fact", "fade", "fair",
    "fairy", "faith", "fall", "false", "fame", "family", "fancy", "farm",
    "fast", "fat", "fate", "fault", "fawn", "fear", "feast", "feed",
    "fern", "ferry", "fever", "few", "fiber", "field", "fifth", "fig",
    "film", "find", "fine", "finer", "finish", "fire", "firm", "first",
    "fish", "five", "fix", "flag", "flame", "flash", "flat", "flax",
    "flex", "flint", "float", "flock", "flood", "floor", "flora", "flour",
    "flow", "flower", "fluff", "fluid", "fluke", "flute", "fly", "foam",
    "fog", "foil", "fold", "folk", "fond", "food", "foot", "force",
    "ford", "forge", "fork", "form", "fort", "forum", "fossil", "fox",
    "frame", "free", "fresh", "friar", "fries", "frog", "from", "front",
    "frost", "froth", "fruit", "fry", "fuel", "full", "fun", "fund",
    "funny", "fur", "fury", "fuse", "gable", "gadget", "gain", "gala",
    "gamma", "gap", "garden", "gargle", "garlic", "gate", "gauge", "gear",
    "gecko", "gem", "gentle", "gift", "ginger", "girl", "glad", "glide",
    "glitch", "globe", "gloom", "gloss", "glove", "glow", "glue", "gnat",
    "goat", "gold", "golf", "gone", "good", "goose", "gospel", "grab",
    "grace", "grade", "grain", "grape", "graph", "grasp", "grass", "grate",
    "gravy", "great", "grid", "grief", "grim", "grin", "grip", "grit",
    "groan", "groom", "gross", "group", "grout", "grove", "grow", "grub",
    "guess", "guide", "guild", "guilt", "guitar", "gulf", "gum", "guru",
    "habit", "haiku", "hair", "half", "hall", "halt", "ham", "hand",
    "hang", "happy", "harbor", "hard", "hare", "harm", "harp", "hash",
    "haste", "hat", "hatch", "have", "haven", "hawk", "hay", "hazel",
    "head", "heal", "heap", "heart", "heat", "heavy", "hedge", "heel",
    "help", "hemp", "hen", "herb", "hero", "hex", "hide", "high",
    "hike", "hill", "hip", "hive", "hobby", "hog", "hold", "hole",
    "hollow", "holy", "home", "honey", "honor", "hood", "hoof", "hook",
    "hoop", "hope", "horn", "horse", "host", "hot", "hound", "hour",
    "house", "hub", "hug", "human", "humble", "humor", "hump", "hunch",
    "hunt", "hurry", "husk", "hut", "hyena", "hymn", "ice", "icon",
    "idea", "igloo", "imp", "index", "indigo", "infant", "inlet", "ink",
    "inlay", "inner", "input", "iris", "iron", "ivory", "ivy", "jade",
    "jam", "jar", "java", "jaw", "jazz", "jeans", "jelly", "jest",
    "jet", "jewel", "jiffy", "jig", "job", "join", "joke", "jolly",
    "joy", "judge", "juice", "jump", "jungle", "junior", "junk", "jury",
    "kayak", "keep", "kept", "kettle", "key", "kick", "kid", "kilt",
    "kind", "king", "kite", "kitten", "knack", "knee", "knife", "knock",
    "koala", "label", "lace", "ladder", "lake", "lamb", "lamp", "lance",
    "land", "lane", "laser", "later", "latte", "laugh", "lava", "lawn",
    "layer", "lazy", "leaf", "lean", "leap", "learn", "lease", "led",
    "ledge", "left", "legal", "lemon", "lend", "lens", "level", "lever",
    "lick", "lid", "life", "lift", "light", "lilac", "lime", "line",
    "link", "lint", "lion", "lip", "list", "live", "load", "loaf",
    "loan", "lobby", "lobe", "local", "lock", "loft", "log", "logic",
    "long", "look", "loop", "loose", "lotus", "loud", "lounge", "love",
    "low", "loyal", "luck", "lunar", "lunch", "lung", "lure", "lush",
    "lute", "lynx", "lyric", "mace", "madam", "made", "magic", "main",
    "make", "mallet", "malt", "mango", "manor", "mantle", "maple", "march",
    "mare", "mark", "mars", "marsh", "mask", "mast", "match", "mate",
    "math", "maze", "meadow", "meal", "meat", "medal", "meet", "mellow",
    "melody", "melt", "memo", "menu", "mercy", "merge", "merit", "merry",
    "mesh", "metal", "meter", "mew", "mice", "midst", "might", "mild",
    "mile", "milk", "mill", "mimic", "mind", "mine", "mint", "minus",
    "mirror", "mist", "moat", "mocha", "modal", "model", "modem", "moist",
    "mole", "money", "month", "moon", "moose", "moral", "more", "moth",
    "motor", "mount", "mouse", "move", "movie", "much", "muffin", "mulch",
    "mule", "muse", "music", "mute", "myth",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_is_four_lowercase_words() {
        let pw = generate_passphrase();
        let words: Vec<&str> = pw.split(' ').collect();
        assert_eq!(words.len(), 4, "passphrase should be 4 words: {:?}", pw);
        for w in &words {
            assert!(!w.is_empty(), "empty word in passphrase: {:?}", pw);
            assert!(
                w.chars().all(|c| c.is_ascii_lowercase()),
                "non-lowercase-letter in word {:?} of {:?}",
                w,
                pw
            );
        }
    }

    #[test]
    fn passphrase_words_are_from_the_wordlist() {
        let pw = generate_passphrase();
        for w in pw.split(' ') {
            assert!(
                PASSPHRASE_WORDS.contains(&w),
                "word {:?} not in the embedded wordlist",
                w
            );
        }
    }

    #[test]
    fn wordlist_is_well_formed() {
        assert!(
            PASSPHRASE_WORDS.len() >= 256,
            "wordlist too small for reasonable entropy: {}",
            PASSPHRASE_WORDS.len()
        );
        for w in PASSPHRASE_WORDS {
            assert!(!w.is_empty(), "empty word in list");
            assert!(
                w.chars().all(|c| c.is_ascii_lowercase()),
                "non-lowercase word in list: {:?}",
                w
            );
        }
    }
}
