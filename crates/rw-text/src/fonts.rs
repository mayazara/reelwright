//! The fonts text can use: the bundled set first, then the computer's own
//! fonts and any in the user's fonts folder.

use cosmic_text::fontdb;
use cosmic_text::{Fallback, FontSystem, PlatformFallback};
use std::path::PathBuf;
use std::sync::Arc;
use unicode_script::Script;

macro_rules! bundled {
    ($($path:literal),* $(,)?) => {
        &[$(include_bytes!(concat!("../../../assets/fonts/", $path)) as &[u8]),*]
    };
}

/// Every font file shipped with the app.
const BUNDLED: &[&[u8]] = bundled!(
    "title/abrilfatface/AbrilFatface-Regular.ttf",
    "title/anton/Anton-Regular.ttf",
    "title/archivoblack/ArchivoBlack-Regular.ttf",
    "title/bangers/Bangers-Regular.ttf",
    "title/bebasneue/BebasNeue-Regular.ttf",
    "title/caveat/Caveat-Variable.ttf",
    "title/courierprime/CourierPrime-Bold.ttf",
    "title/courierprime/CourierPrime-Regular.ttf",
    "title/crimsonpro/CrimsonPro-Variable.ttf",
    "title/dancingscript/DancingScript-Variable.ttf",
    "title/dmsans/DMSans-Variable.ttf",
    "title/dmserifdisplay/DMSerifDisplay-Regular.ttf",
    "title/ebgaramond/EBGaramond-Variable.ttf",
    "title/figtree/Figtree-Variable.ttf",
    "title/fraunces/Fraunces-Variable.ttf",
    "title/librebaskerville/LibreBaskerville-Variable.ttf",
    "title/lobster/Lobster-Regular.ttf",
    "title/lora/Lora-Variable.ttf",
    "title/manrope/Manrope-Variable.ttf",
    "title/monoton/Monoton-Regular.ttf",
    "title/montserrat/Montserrat-Variable.ttf",
    "title/notosansarabic/NotoSansArabic-Variable.ttf",
    "title/notosansdevanagari/NotoSansDevanagari-Variable.ttf",
    "title/nunito/Nunito-Variable.ttf",
    "title/oswald/Oswald-Variable.ttf",
    "title/outfit/Outfit-Variable.ttf",
    "title/pacifico/Pacifico-Regular.ttf",
    "title/playfairdisplay/PlayfairDisplay-Variable.ttf",
    "title/poppins/Poppins-Bold.ttf",
    "title/poppins/Poppins-Regular.ttf",
    "title/pressstart2p/PressStart2P-Regular.ttf",
    "title/raleway/Raleway-Variable.ttf",
    "title/righteous/Righteous-Regular.ttf",
    "title/spacemono/SpaceMono-Bold.ttf",
    "title/spacemono/SpaceMono-Regular.ttf",
    "title/worksans/WorkSans-Variable.ttf",
    "ui/Inter-Regular.ttf",
    "ui/Inter-SemiBold.ttf",
);

/// Bundled fonts that cover scripts the title fonts don't; tried first for
/// those scripts so text looks the same on every computer.
const ARABIC: &str = "Noto Sans Arabic";
const DEVANAGARI: &str = "Noto Sans Devanagari";

/// Bundled fonts first for the scripts we ship fonts for, then the
/// platform's usual choices.
struct BundledFirst {
    platform: PlatformFallback,
    arabic: &'static [&'static str],
    devanagari: &'static [&'static str],
}

impl BundledFirst {
    fn new(locale: &str) -> Self {
        let platform = PlatformFallback;
        let with = |first: &'static str, script: Script| -> &'static [&'static str] {
            let mut v = vec![first];
            v.extend(platform.script_fallback(script, locale).iter().copied());
            Box::leak(v.into_boxed_slice())
        };
        BundledFirst {
            arabic: with(ARABIC, Script::Arabic),
            devanagari: with(DEVANAGARI, Script::Devanagari),
            platform,
        }
    }
}

impl Fallback for BundledFirst {
    fn common_fallback(&self) -> &[&'static str] {
        self.platform.common_fallback()
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        self.platform.forbidden_fallback()
    }

    fn script_fallback(&self, script: Script, locale: &str) -> &[&'static str] {
        match script {
            Script::Arabic => self.arabic,
            Script::Devanagari => self.devanagari,
            _ => self.platform.script_fallback(script, locale),
        }
    }
}

/// Extra folders to load fonts from, separated like `PATH`
/// (`RW_FONT_DIRS`); used by tests and for portable installs.
fn extra_dirs() -> Vec<PathBuf> {
    std::env::var_os("RW_FONT_DIRS")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default()
}

/// Builds the font database: bundled fonts, system fonts, then `user_dir`
/// (the app's fonts folder) and `RW_FONT_DIRS`.
pub fn font_system(user_dir: Option<&std::path::Path>) -> FontSystem {
    let mut db = fontdb::Database::new();
    for data in BUNDLED {
        db.load_font_source(fontdb::Source::Binary(Arc::new(*data)));
    }
    db.load_system_fonts();
    for dir in user_dir.map(PathBuf::from).into_iter().chain(extra_dirs()) {
        db.load_fonts_dir(dir);
    }
    db.set_sans_serif_family("Inter");
    db.set_serif_family("Lora");
    db.set_monospace_family("Space Mono");
    let locale = "en-US".to_owned();
    let fallback = BundledFirst::new(&locale);
    FontSystem::new_with_locale_and_db_and_fallback(locale, db, fallback)
}

/// Family names of the bundled fonts, for pickers (title fonts only).
pub fn bundled_families(system: &FontSystem) -> Vec<String> {
    let mut names: Vec<String> = system
        .db()
        .faces()
        .filter(|f| matches!(f.source, fontdb::Source::Binary(_)))
        .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .filter(|n| n != ARABIC && n != DEVANAGARI && !n.starts_with("Inter"))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Family names of fonts installed on this computer (not bundled).
pub fn system_families(system: &FontSystem) -> Vec<String> {
    let mut names: Vec<String> = system
        .db()
        .faces()
        .filter(|f| !matches!(f.source, fontdb::Source::Binary(_)))
        .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}
