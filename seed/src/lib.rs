#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

use core::{fmt, ops::Deref};
use std_shims::{
    collections::HashMap,
    string::{String, ToString},
    sync::LazyLock,
    vec,
    vec::Vec,
};

use rand_core::{CryptoRng, RngCore};
use zeroize::{Zeroize, Zeroizing};

use curve25519_dalek::scalar::Scalar;

#[cfg(test)]
mod tests;

// The amount of words in a seed without a checksum.
const SEED_LENGTH: usize = 24;
// The amount of words in a seed with a checksum.
const SEED_LENGTH_WITH_CHECKSUM: usize = 25;

/// An error when working with a seed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
pub enum SeedError {
    #[cfg_attr(feature = "std", error("invalid seed"))]
    /// The seed was invalid.
    InvalidSeed,
    /// The checksum did not match the data.
    #[cfg_attr(feature = "std", error("invalid checksum"))]
    InvalidChecksum,
    /// The deprecated English language option was used with a checksum.
    ///
    /// The deprecated English language option did not include a checksum.
    #[cfg_attr(
        feature = "std",
        error("deprecated English language option included a checksum")
    )]
    DeprecatedEnglishWithChecksum,
}

/// Language options.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Zeroize)]
pub enum Language {
    /// Chinese language option.
    Chinese,
    /// English language option.
    English,
    /// Dutch language option.
    Dutch,
    /// French language option.
    French,
    /// Spanish language option.
    Spanish,
    /// German language option.
    German,
    /// Italian language option.
    Italian,
    /// Portuguese language option.
    Portuguese,
    /// Japanese language option.
    Japanese,
    /// Russian language option.
    Russian,
    /// Esperanto language option.
    Esperanto,
    /// Lojban language option.
    Lojban,
    /// The original, and deprecated, English language.
    DeprecatedEnglish,
}

fn trim(word: &str, len: usize) -> Zeroizing<String> {
    Zeroizing::new(word.chars().take(len).collect())
}

struct WordList {
    word_list: &'static [&'static str],
    word_map: HashMap<&'static str, usize>,
    trimmed_word_map: HashMap<String, usize>,
    unique_prefix_length: usize,
}

impl WordList {
    fn new(word_list: &'static [&'static str], prefix_length: usize) -> WordList {
        let mut lang = WordList {
            word_list,
            word_map: HashMap::new(),
            trimmed_word_map: HashMap::new(),
            unique_prefix_length: prefix_length,
        };

        for (i, word) in lang.word_list.iter().enumerate() {
            lang.word_map.insert(word, i);
            lang.trimmed_word_map
                .insert(trim(word, lang.unique_prefix_length).deref().clone(), i);
        }

        lang
    }
}

static LANGUAGES: LazyLock<HashMap<Language, WordList>> = LazyLock::new(|| {
    HashMap::from([
        (
            Language::Chinese,
            WordList::new(include!("./words/zh.rs"), 1),
        ),
        (
            Language::English,
            WordList::new(include!("./words/en.rs"), 3),
        ),
        (Language::Dutch, WordList::new(include!("./words/nl.rs"), 4)),
        (
            Language::French,
            WordList::new(include!("./words/fr.rs"), 4),
        ),
        (
            Language::Spanish,
            WordList::new(include!("./words/es.rs"), 4),
        ),
        (
            Language::German,
            WordList::new(include!("./words/de.rs"), 4),
        ),
        (
            Language::Italian,
            WordList::new(include!("./words/it.rs"), 4),
        ),
        (
            Language::Portuguese,
            WordList::new(include!("./words/pt.rs"), 4),
        ),
        (
            Language::Japanese,
            WordList::new(include!("./words/ja.rs"), 3),
        ),
        (
            Language::Russian,
            WordList::new(include!("./words/ru.rs"), 4),
        ),
        (
            Language::Esperanto,
            WordList::new(include!("./words/eo.rs"), 4),
        ),
        (
            Language::Lojban,
            WordList::new(include!("./words/jbo.rs"), 4),
        ),
        (
            Language::DeprecatedEnglish,
            WordList::new(include!("./words/ang.rs"), 4),
        ),
    ])
});

fn checksum_index(words: &[Zeroizing<String>], lang: &WordList) -> usize {
    let mut trimmed_words = Zeroizing::new(String::new());
    for w in words {
        *trimmed_words += &trim(w, lang.unique_prefix_length);
    }

    const fn crc32_table() -> [u32; 256] {
        let poly = 0xedb88320u32;

        let mut res = [0; 256];
        let mut i = 0;
        while i < 256 {
            let mut entry = i;
            let mut b = 0;
            while b < 8 {
                let trigger = entry & 1;
                entry >>= 1;
                if trigger == 1 {
                    entry ^= poly;
                }
                b += 1;
            }
            res[i as usize] = entry;
            i += 1;
        }

        res
    }
    const CRC32_TABLE: [u32; 256] = crc32_table();

    let trimmed_words = trimmed_words.as_bytes();
    let mut checksum = u32::MAX;
    for i in 0..trimmed_words.len() {
        checksum = CRC32_TABLE
            [usize::from(u8::try_from(checksum % 256).unwrap() ^ trimmed_words[i])]
            ^ (checksum >> 8);
    }

    usize::try_from(!checksum).unwrap() % words.len()
}

// Convert a private key to a seed
#[allow(clippy::needless_pass_by_value)]
fn key_to_seed(lang: Language, key: Zeroizing<Scalar>) -> Seed {
    let bytes = Zeroizing::new(key.to_bytes());

    // get the language words
    let words = &LANGUAGES[&lang].word_list;
    let list_len = u64::try_from(words.len()).unwrap();

    // To store the found words & add the checksum word later.
    let mut seed = Vec::with_capacity(25);

    // convert to words
    // 4 bytes -> 3 words. 8 digits base 16 -> 3 digits base 1626
    let mut segment = [0; 4];
    let mut indices = [0; 4];
    for i in 0..8 {
        // convert first 4 byte to u32 & get the word indices
        let start = i * 4;
        // convert 4 byte to u32
        segment.copy_from_slice(&bytes[start..(start + 4)]);
        // Actually convert to a u64 so we can add without overflowing
        indices[0] = u64::from(u32::from_le_bytes(segment));
        indices[1] = indices[0];
        indices[0] /= list_len;
        indices[2] = indices[0] + indices[1];
        indices[0] /= list_len;
        indices[3] = indices[0] + indices[2];

        // append words to seed
        for i in indices.iter().skip(1) {
            let word = usize::try_from(i % list_len).unwrap();
            seed.push(Zeroizing::new(words[word].to_string()));
        }
    }
    segment.zeroize();
    indices.zeroize();

    // create a checksum word for all languages except old english
    if lang != Language::DeprecatedEnglish {
        let checksum = seed[checksum_index(&seed, &LANGUAGES[&lang])].clone();
        seed.push(checksum);
    }

    let mut res = Zeroizing::new(String::new());
    for (i, word) in seed.iter().enumerate() {
        if i != 0 {
            *res += " ";
        }
        *res += word;
    }
    Seed(lang, res)
}

// Convert a seed to bytes
fn seed_to_bytes(lang: Language, words: &str) -> Result<Zeroizing<[u8; 32]>, SeedError> {
    // get seed words
    let words = words
        .split_whitespace()
        .map(|w| Zeroizing::new(w.to_string()))
        .collect::<Vec<_>>();
    if (words.len() != SEED_LENGTH) && (words.len() != SEED_LENGTH_WITH_CHECKSUM) {
        panic!("invalid seed passed to seed_to_bytes");
    }

    let has_checksum = words.len() == SEED_LENGTH_WITH_CHECKSUM;
    if has_checksum && lang == Language::DeprecatedEnglish {
        Err(SeedError::DeprecatedEnglishWithChecksum)?;
    }

    // Validate words are in the language word list
    let lang_word_list: &WordList = &LANGUAGES[&lang];
    let matched_indices = (|| {
        let has_checksum = words.len() == SEED_LENGTH_WITH_CHECKSUM;
        let mut matched_indices = Zeroizing::new(vec![]);

        // Iterate through all the words and see if they're all present
        for word in &words {
            let trimmed = trim(word, lang_word_list.unique_prefix_length);
            let word = if has_checksum { &trimmed } else { word };

            if let Some(index) = if has_checksum {
                lang_word_list.trimmed_word_map.get(word.deref())
            } else {
                lang_word_list.word_map.get(&word.as_str())
            } {
                matched_indices.push(*index);
            } else {
                Err(SeedError::InvalidSeed)?;
            }
        }

        if has_checksum {
            // exclude the last word when calculating a checksum.
            let last_word = words.last().unwrap().clone();
            let checksum = words[checksum_index(&words[..words.len() - 1], lang_word_list)].clone();

            // check the trimmed checksum and trimmed last word line up
            if trim(&checksum, lang_word_list.unique_prefix_length)
                != trim(&last_word, lang_word_list.unique_prefix_length)
            {
                Err(SeedError::InvalidChecksum)?;
            }
        }

        Ok(matched_indices)
    })()?;

    // convert to bytes
    let mut res = Zeroizing::new([0; 32]);
    let mut indices = Zeroizing::new([0; 4]);
    for i in 0..8 {
        // read 3 indices at a time
        let i3 = i * 3;
        indices[1] = matched_indices[i3];
        indices[2] = matched_indices[i3 + 1];
        indices[3] = matched_indices[i3 + 2];

        let inner = |i| {
            let mut base = (lang_word_list.word_list.len() - indices[i] + indices[i + 1])
                % lang_word_list.word_list.len();
            // Shift the index over
            for _ in 0..i {
                base *= lang_word_list.word_list.len();
            }
            base
        };
        // set the last index
        indices[0] = indices[1] + inner(1) + inner(2);
        if (indices[0] % lang_word_list.word_list.len()) != indices[1] {
            Err(SeedError::InvalidSeed)?;
        }

        let pos = i * 4;
        let mut bytes = u32::try_from(indices[0]).unwrap().to_le_bytes();
        res[pos..(pos + 4)].copy_from_slice(&bytes);
        bytes.zeroize();
    }

    Ok(res)
}

/// A Monero seed.
#[derive(Clone, PartialEq, Eq, Zeroize)]
pub struct Seed(Language, Zeroizing<String>);

impl fmt::Debug for Seed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Seed").finish_non_exhaustive()
    }
}

impl Seed {
    /// Create a new seed.
    pub fn new<R: RngCore + CryptoRng>(rng: &mut R, lang: Language) -> Seed {
        let mut scalar_bytes = Zeroizing::new([0; 64]);
        rng.fill_bytes(scalar_bytes.as_mut());
        key_to_seed(
            lang,
            Zeroizing::new(Scalar::from_bytes_mod_order_wide(scalar_bytes.deref())),
        )
    }

    /// Parse a seed from a string.
    #[allow(clippy::needless_pass_by_value)]
    pub fn from_string(lang: Language, words: Zeroizing<String>) -> Result<Seed, SeedError> {
        let entropy = seed_to_bytes(lang, &words)?;

        // Make sure this is a valid scalar
        let scalar = Scalar::from_canonical_bytes(*entropy);
        if scalar.is_none().into() {
            Err(SeedError::InvalidSeed)?;
        }
        let mut scalar = scalar.unwrap();
        scalar.zeroize();

        // Call from_entropy so a trimmed seed becomes a full seed
        Ok(Self::from_entropy(lang, entropy).unwrap())
    }

    /// Create a seed from entropy.
    #[allow(clippy::needless_pass_by_value)]
    pub fn from_entropy(lang: Language, entropy: Zeroizing<[u8; 32]>) -> Option<Seed> {
        Option::from(Scalar::from_canonical_bytes(*entropy))
            .map(|scalar| key_to_seed(lang, Zeroizing::new(scalar)))
    }

    /// Convert a seed to a string.
    pub fn to_string(&self) -> Zeroizing<String> {
        self.1.clone()
    }

    /// Return the entropy underlying this seed.
    pub fn entropy(&self) -> Zeroizing<[u8; 32]> {
        seed_to_bytes(self.0, &self.1).unwrap()
    }
}
