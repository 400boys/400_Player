//! Interface translations: French and English.
//!
//! Every string is a variant of [`Key`] declared once, with both translations, in the
//! `translations!` table below — adding a key without translating it into *both* languages does
//! not compile. Placeholders are written `{0}`, `{1}`… and filled by [`tf`]; a unit test checks that
//! both languages use the same placeholders. French typography (no-break space before `:`, `?`,
//! `!`, `;`, `%` and inside « guillemets ») is applied automatically.

use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use media_player_400_core::settings::Language;

macro_rules! translations {
    ($( $name:ident => ($fr:expr, $en:expr $(,)?) ),* $(,)?) => {
        /// Identifier of a translated string.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Key { $( $name ),* }

        impl Key {
            /// Every key, in declaration order.
            pub const ALL: &'static [Key] = &[ $( Key::$name ),* ];

            fn raw_fr(self) -> &'static str {
                match self { $( Key::$name => $fr ),* }
            }

            fn raw_en(self) -> &'static str {
                match self { $( Key::$name => $en ),* }
            }
        }
    };
}

static LANG: AtomicU8 = AtomicU8::new(0);

/// Tests that read or change the language take this lock so they do not race each other.
#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Sets the interface language (`Auto` is resolved against the system locale).
pub fn set_language(l: Language) {
    let resolved = match l {
        Language::Auto => {
            let locale = sys_locale::get_locale().unwrap_or_default();
            Language::Auto.resolve(&locale)
        }
        other => other,
    };
    LANG.store(u8::from(resolved == Language::French), Ordering::Relaxed);
}

/// The language in use (never `Auto`).
pub fn current() -> Language {
    if LANG.load(Ordering::Relaxed) == 1 { Language::French } else { Language::English }
}

/// Applies French typographic rules: a no-break space before `: ; ? ! %` and inside guillemets.
pub fn french_typography(s: &str) -> String {
    const NBSP: char = '\u{a0}';
    let mut out = String::with_capacity(s.len() + 4);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ' '
            && let Some(':' | ';' | '?' | '!' | '%' | '»') = chars.peek()
        {
            out.push(NBSP);
            continue;
        }
        out.push(c);
        if c == '«' && chars.peek() == Some(&' ') {
            chars.next();
            out.push(NBSP);
        }
    }
    out
}

fn fr_table() -> &'static [&'static str] {
    static TABLE: OnceLock<Vec<&'static str>> = OnceLock::new();
    TABLE.get_or_init(|| Key::ALL.iter().map(|k| &*Box::leak(french_typography(k.raw_fr()).into_boxed_str())).collect())
}

/// The text of `k` in the current language.
pub fn t(k: Key) -> &'static str {
    if LANG.load(Ordering::Relaxed) == 1 { fr_table()[k as usize] } else { k.raw_en() }
}

/// The text of `k` with `{0}`, `{1}`… replaced by `args`.
pub fn tf(k: Key, args: &[&dyn Display]) -> String {
    let mut s = t(k).to_owned();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), &a.to_string());
    }
    s
}

/// Shorthand for one argument.
pub fn t1(k: Key, a: impl Display) -> String {
    tf(k, &[&a])
}

/// The text of `k` in a given language, whatever the interface language is (tests).
#[cfg(test)]
pub fn t_in(k: Key, l: Language) -> &'static str {
    match l {
        Language::French => fr_table()[k as usize],
        _ => k.raw_en(),
    }
}

translations! {
    // ---- application -------------------------------------------------------------------------
    AppName => ("400_Player", "400_Player"),
    Tagline => ("Le lecteur multimédia, réinventé.", "The media player, reimagined."),
    Untitled => ("Sans titre", "Untitled"),

    // ---- welcome screen ----------------------------------------------------------------------
    OpenFile => ("Ouvrir un fichier", "Open a file"),
    OpenFiles => ("Ouvrir des fichiers…", "Open files…"),
    OpenFolder => ("Ouvrir un dossier…", "Open a folder…"),
    OpenUrl => ("Ouvrir une adresse réseau…", "Open a network URL…"),
    OpenUrlTitle => ("Ouvrir une adresse réseau", "Open a network URL"),
    OpenUrlHint => ("http://, https://, rtsp://, … ou le chemin d'un fichier", "http://, https://, rtsp://, … or a file path"),
    OpenUrlConfirm => ("Ouvrir", "Open"),
    Cancel => ("Annuler", "Cancel"),
    Close => ("Fermer", "Close"),
    Recent => ("Récemment lus", "Recently played"),
    NoRecent => ("Rien n'a encore été lu. Glissez un fichier ici pour commencer.", "Nothing has been played yet. Drop a file here to begin."),
    DropHint => ("Déposez un fichier, un dossier ou un lien", "Drop a file, a folder or a link"),
    DropToPlay => ("Relâchez pour lire", "Release to play"),
    ClearHistory => ("Effacer l'historique", "Clear history"),
    RemoveFromList => ("Retirer de la liste", "Remove from the list"),
    ResumeAt => ("Reprendre à {0}", "Resume at {0}"),
    WatchedPercent => ("{0} % vu", "{0} % watched"),
    PlayAgain => ("Rejouer", "Play again"),

    // ---- transport ---------------------------------------------------------------------------
    Play => ("Lecture", "Play"),
    Pause => ("Pause", "Pause"),
    Stop => ("Arrêter", "Stop"),
    Next => ("Suivant", "Next"),
    Previous => ("Précédent", "Previous"),
    SeekBack => ("Reculer de {0} s", "Back {0} s"),
    SeekForward => ("Avancer de {0} s", "Forward {0} s"),
    FrameNext => ("Image suivante", "Next frame"),
    FramePrevious => ("Image précédente", "Previous frame"),
    Fullscreen => ("Plein écran", "Fullscreen"),
    ExitFullscreen => ("Quitter le plein écran", "Exit fullscreen"),
    Mute => ("Couper le son", "Mute"),
    Unmute => ("Rétablir le son", "Unmute"),
    Volume => ("Volume", "Volume"),
    VolumeValue => ("Volume : {0} %", "Volume: {0} %"),
    Muted => ("Son coupé", "Muted"),
    Speed => ("Vitesse", "Speed"),
    SpeedValue => ("Vitesse : ×{0}", "Speed: ×{0}"),
    SpeedNormal => ("Normale", "Normal"),
    Buffering => ("Mise en mémoire tampon…", "Buffering…"),
    Opening => ("Ouverture…", "Opening…"),
    Live => ("EN DIRECT", "LIVE"),
    Elapsed => ("Temps écoulé", "Elapsed time"),
    Remaining => ("Temps restant", "Time remaining"),
    Chapter => ("Chapitre", "Chapter"),
    ChapterN => ("Chapitre {0}", "Chapter {0}"),
    NextChapter => ("Chapitre suivant", "Next chapter"),
    PreviousChapter => ("Chapitre précédent", "Previous chapter"),

    // ---- tracks and subtitles ----------------------------------------------------------------
    Subtitles => ("Sous-titres", "Subtitles"),
    SubtitlesOff => ("Désactivés", "Off"),
    SubtitlesValue => ("Sous-titres : {0}", "Subtitles: {0}"),
    LoadSubtitles => ("Charger un fichier de sous-titres…", "Load a subtitle file…"),
    SubtitleTrackN => ("Sous-titres {0}", "Subtitles {0}"),
    SubtitleExternal => ("Fichier externe", "External file"),
    SubtitleBitmap => ("image", "bitmap"),
    SubtitleForced => ("forcés", "forced"),
    SubtitleDelay => ("Décalage des sous-titres", "Subtitle delay"),
    SubtitleDelayValue => ("Décalage des sous-titres : {0}", "Subtitle delay: {0}"),
    AudioTrack => ("Piste audio", "Audio track"),
    AudioTrackValue => ("Piste audio : {0}", "Audio track: {0}"),
    AudioTrackN => ("Piste {0}", "Track {0}"),
    AudioDelay => ("Décalage audio", "Audio delay"),
    AudioDelayValue => ("Décalage audio : {0}", "Audio delay: {0}"),
    NoAudio => ("Aucune piste audio", "No audio track"),
    VideoTrack => ("Piste vidéo", "Video track"),
    NoTracks => ("Aucune autre piste", "No other track"),
    Default => ("par défaut", "default"),
    Channels => ("{0} canaux", "{0} channels"),
    Mono => ("mono", "mono"),
    Stereo => ("stéréo", "stereo"),

    // ---- picture -----------------------------------------------------------------------------
    AspectRatio => ("Format de l'image", "Aspect ratio"),
    AspectAuto => ("Automatique", "Automatic"),
    AspectFill => ("Remplir l'écran (rogner)", "Fill the screen (crop)"),
    AspectStretch => ("Étirer", "Stretch"),
    AspectOriginal => ("Taille d'origine", "Original size"),
    AspectValue => ("Format : {0}", "Aspect: {0}"),
    Zoom => ("Zoom", "Zoom"),
    ZoomValue => ("Zoom : {0} %", "Zoom: {0} %"),
    ZoomIn => ("Zoom avant", "Zoom in"),
    ZoomOut => ("Zoom arrière", "Zoom out"),
    ZoomReset => ("Zoom à 100 %", "Reset zoom"),
    RotateRight => ("Pivoter à droite", "Rotate right"),
    FlipHorizontal => ("Miroir horizontal", "Flip horizontally"),
    ResetPicture => ("Réinitialiser l'image", "Reset the picture"),
    Picture => ("Image", "Picture"),
    Brightness => ("Luminosité", "Brightness"),
    Contrast => ("Contraste", "Contrast"),
    Saturation => ("Saturation", "Saturation"),
    Gamma => ("Gamma", "Gamma"),
    AmbientGlow => ("Éclairage d'ambiance", "Ambient glow"),
    AmbientOn => ("Éclairage d'ambiance activé", "Ambient glow on"),
    AmbientOff => ("Éclairage d'ambiance désactivé", "Ambient glow off"),
    Screenshot => ("Capture d'écran", "Take a screenshot"),
    ScreenshotSaved => ("Capture enregistrée : {0}", "Screenshot saved: {0}"),
    ScreenshotFailed => ("Impossible d'enregistrer la capture : {0}", "Could not save the screenshot: {0}"),
    ScreenshotNoVideo => ("Aucune image à capturer", "No picture to capture"),
    ShowInFolder => ("Afficher dans le dossier", "Show in folder"),

    // ---- playlist ----------------------------------------------------------------------------
    Playlist => ("Liste de lecture", "Playlist"),
    PlaylistEmpty => ("La liste est vide", "The playlist is empty"),
    PlaylistEmptyHint => ("Ajoutez des fichiers ou déposez-les ici.", "Add files or drop them here."),
    PlaylistCount => ("{0} éléments", "{0} items"),
    PlaylistCountOne => ("1 élément", "1 item"),
    PlaylistTotal => ("{0} éléments · {1}", "{0} items · {1}"),
    Add => ("Ajouter", "Add"),
    AddFiles => ("Ajouter des fichiers…", "Add files…"),
    AddFolder => ("Ajouter un dossier…", "Add a folder…"),
    AddUrl => ("Ajouter une adresse…", "Add a URL…"),
    Clear => ("Vider", "Clear"),
    ClearPlaylist => ("Vider la liste", "Clear the playlist"),
    SortByName => ("Trier par nom", "Sort by name"),
    Shuffle => ("Lecture aléatoire", "Shuffle"),
    ShuffleOn => ("Lecture aléatoire activée", "Shuffle on"),
    ShuffleOff => ("Lecture aléatoire désactivée", "Shuffle off"),
    Repeat => ("Répéter", "Repeat"),
    RepeatOff => ("Répétition désactivée", "Repeat off"),
    RepeatAll => ("Répétition de toute la liste", "Repeat all"),
    RepeatOne => ("Répétition du titre en cours", "Repeat one"),
    PlayNow => ("Lire maintenant", "Play now"),
    RemoveItem => ("Retirer de la liste", "Remove from the playlist"),
    SavePlaylist => ("Enregistrer la liste…", "Save the playlist…"),
    LoadPlaylist => ("Charger une liste…", "Load a playlist…"),
    PlaylistSaved => ("Liste enregistrée : {0}", "Playlist saved: {0}"),
    PlaylistEnded => ("Fin de la liste de lecture", "End of the playlist"),
    NowPlaying => ("En cours", "Now playing"),
    UpNext => ("À suivre", "Up next"),

    // ---- loop, jump ---------------------------------------------------------------------------
    ABLoop => ("Boucle A–B", "A–B loop"),
    ABLoopA => ("Boucle : début défini à {0}", "Loop: start set at {0}"),
    ABLoopOn => ("Boucle {0} → {1}", "Loop {0} → {1}"),
    ABLoopOff => ("Boucle désactivée", "Loop cleared"),
    JumpToTime => ("Aller à un instant…", "Jump to time…"),
    JumpToTimeHint => ("Par exemple 1:23:45, 90, 1h2m3s", "For example 1:23:45, 90, 1h2m3s"),
    JumpInvalid => ("Temps non reconnu", "Time not recognised"),
    Go => ("Aller", "Go"),

    // ---- window ------------------------------------------------------------------------------
    AlwaysOnTop => ("Toujours au premier plan", "Always on top"),
    Minimize => ("Réduire", "Minimise"),
    Maximize => ("Agrandir", "Maximise"),
    Restore => ("Restaurer", "Restore"),
    Quit => ("Quitter", "Quit"),
    More => ("Plus d'options", "More options"),
    Settings => ("Réglages", "Settings"),
    MediaInfo => ("Informations sur le média", "Media information"),
    Statistics => ("Statistiques de lecture", "Playback statistics"),
    Shortcuts => ("Raccourcis clavier", "Keyboard shortcuts"),
    CommandPalette => ("Palette de commandes", "Command palette"),
    PaletteHint => ("Rechercher une action, un fichier récent…", "Search an action, a recent file…"),
    PaletteNoResult => ("Aucun résultat", "No result"),
    Equalizer => ("Égaliseur", "Equaliser"),
    ShowRemaining => ("Afficher le temps restant", "Show remaining time"),
    ToggleControls => ("Afficher les commandes", "Show controls"),

    // ---- messages ----------------------------------------------------------------------------
    CannotOpen => ("Impossible d'ouvrir « {0} »", "Cannot open \"{0}\""),
    NoSoundCard => ("Aucune sortie audio disponible : lecture silencieuse", "No audio output available: playing silently"),
    AudioDeviceLost => ("Périphérique audio perdu, nouvelle tentative…", "Audio device lost, retrying…"),
    NothingToPlay => ("Aucun fichier lisible trouvé", "No playable file found"),
    UnsupportedFile => ("Fichier ignoré : {0}", "File skipped: {0}"),
    PlaybackFailed => ("La lecture a échoué", "Playback failed"),
    LoadedSubtitles => ("Sous-titres chargés : {0}", "Subtitles loaded: {0}"),
    SubtitleLoadFailed => ("Sous-titres illisibles : {0}", "Could not read the subtitles: {0}"),
    CopiedToClipboard => ("Copié dans le presse-papiers", "Copied to the clipboard"),
    Copy => ("Copier", "Copy"),
    SettingsSaved => ("Réglages enregistrés", "Settings saved"),
    Yes => ("Oui", "Yes"),
    No => ("Non", "No"),
    On => ("activé", "on"),
    Off => ("désactivé", "off"),
    Unknown => ("inconnu", "unknown"),

    // ---- shortcut groups and action labels -----------------------------------------------------
    GroupPlayback => ("Lecture", "Playback"),
    GroupNavigation => ("Navigation", "Navigation"),
    GroupAudio => ("Audio", "Audio"),
    GroupSubtitles => ("Sous-titres", "Subtitles"),
    GroupPicture => ("Image", "Picture"),
    GroupFiles => ("Fichiers", "Files"),
    GroupWindow => ("Fenêtre et interface", "Window and interface"),
    ActPlayPause => ("Lecture / pause", "Play / pause"),
    ActSpeedUp => ("Accélérer la lecture", "Play faster"),
    ActSpeedDown => ("Ralentir la lecture", "Play slower"),
    ActSpeedReset => ("Vitesse normale", "Normal speed"),
    ActRepeat => ("Changer le mode de répétition", "Cycle the repeat mode"),
    ActVolumeUp => ("Monter le volume", "Volume up"),
    ActVolumeDown => ("Baisser le volume", "Volume down"),
    ActMute => ("Couper / rétablir le son", "Mute / unmute"),
    ActCycleAudio => ("Piste audio suivante", "Next audio track"),
    ActCycleSubtitles => ("Sous-titres suivants", "Next subtitle track"),
    ActFullscreen => ("Basculer le plein écran", "Toggle fullscreen"),
    ActCycleAspect => ("Changer le format de l'image", "Cycle the aspect ratio"),
    ActCycleFit => ("Changer le mode d'ajustement", "Cycle the fit mode"),
    ActCycleDeinterlace => ("Changer le désentrelacement", "Cycle deinterlacing"),
    ActAudioEarlier => ("Son plus tôt ({0})", "Audio earlier ({0})"),
    ActAudioLater => ("Son plus tard ({0})", "Audio later ({0})"),
    ActSubtitleEarlier => ("Sous-titres plus tôt ({0})", "Subtitles earlier ({0})"),
    ActSubtitleLater => ("Sous-titres plus tard ({0})", "Subtitles later ({0})"),

    // ---- menu, palette, lists ------------------------------------------------------------------
    Open => ("Ouvrir", "Open"),
    Tools => ("Outils", "Tools"),
    Reset => ("Réinitialiser", "Reset"),
    Confirm => ("Confirmer", "Confirm"),
    Choose => ("Choisir…", "Choose…"),
    Refresh => ("Actualiser", "Refresh"),
    Automatic => ("Automatique", "Automatic"),
    ABLoopSetEnd => ("Boucle A–B : définir la fin", "A–B loop: set the end"),
    RepeatAllShort => ("Répéter la liste", "Repeat list"),
    RepeatOneShort => ("Répéter le titre", "Repeat item"),
    PictureAdjustments => ("Réglages de l'image", "Picture adjustments"),
    CopyPath => ("Copier le chemin", "Copy path"),
    CopyAll => ("Tout copier", "Copy all"),
    DropAddHint => ("Maintenez Maj pour ajouter à la liste", "Hold Shift to add to the playlist"),
    AddedToPlaylist => ("Ajouté à la liste de lecture ({0})", "Added to the playlist ({0})"),
    NoSubtitles => ("Aucun sous-titre disponible", "No subtitles available"),
    NotSeekable => ("Cette source ne permet pas de se déplacer", "This source cannot be seeked"),
    PlaylistSaveFailed => ("Impossible d'enregistrer la liste", "Could not save the playlist"),
    SubtitleEmpty => ("Le fichier ne contient aucun sous-titre", "The file contains no subtitles"),
    SubtitlesNeedMedia => ("Ouvrez d'abord une vidéo pour lui ajouter des sous-titres", "Open a video first to add subtitles to it"),
    NetworkStream => ("Flux réseau", "Network stream"),
    OpenFolderShort => ("Dossier", "Folder"),
    OpenUrlShort => ("Lien", "Link"),
    PlayFromStart => ("Lire depuis le début", "Play from the start"),
    FilterMedia => ("Tous les médias", "All media"),
    FilterVideo => ("Vidéos", "Video"),
    FilterAudio => ("Audio", "Audio"),
    FilterPlaylist => ("Listes de lecture", "Playlists"),
    FilterSubtitles => ("Sous-titres", "Subtitles"),
    ScreenshotFolder => ("Dossier des captures d'écran", "Screenshot folder"),
    PaletteNavigate => ("naviguer", "navigate"),
    PaletteRun => ("exécuter", "run"),
    SetTheme => ("Thème : {0}", "Theme: {0}"),
    SetAccent => ("Couleur d'accent : {0}", "Accent colour: {0}"),
    SetLanguage => ("Langue : {0}", "Language: {0}"),

    // ---- shortcuts window -----------------------------------------------------------------------
    ShortcutsHint => (
        "Ces raccourcis fonctionnent quand la fenêtre est active. Ctrl+K permet de tout rechercher.",
        "These shortcuts work while the window is active. Ctrl+K searches everything."
    ),
    Mouse => ("Souris", "Mouse"),
    Or => ("ou", "or"),
    GestureClick => ("Clic", "Click"),
    GestureClickDoes => ("Lecture / pause", "Play / pause"),
    GestureDoubleClick => ("Double-clic", "Double-click"),
    GestureDoubleClickDoes => ("Plein écran", "Fullscreen"),
    GestureWheel => ("Molette", "Wheel"),
    GestureWheelDoes => ("Volume", "Volume"),
    GestureCtrlWheel => ("Ctrl + molette", "Ctrl + wheel"),
    GestureCtrlWheelDoes => ("Zoom sur l'image", "Zoom the picture"),
    GestureRightClick => ("Clic droit", "Right click"),
    GestureRightClickDoes => ("Menu contextuel", "Context menu"),
    GestureDrop => ("Glisser-déposer", "Drag and drop"),
    GestureDropDoes => ("Lire les fichiers déposés", "Play the dropped files"),
    GestureShiftDrop => ("Maj + glisser-déposer", "Shift + drag and drop"),
    GestureShiftDropDoes => ("Ajouter à la liste", "Add to the playlist"),

    // ---- appearance ---------------------------------------------------------------------------------
    Appearance => ("Apparence", "Appearance"),
    ThemeDark => ("Sombre", "Dark"),
    ThemeOled => ("OLED", "OLED"),
    ThemeLight => ("Clair", "Light"),
    ThemeSystem => ("Système", "System"),
    AccentViolet => ("Violet", "Violet"),
    AccentSky => ("Ciel", "Sky"),
    AccentEmerald => ("Émeraude", "Emerald"),
    AccentRose => ("Rose", "Rose"),
    AccentAmber => ("Ambre", "Amber"),
    AccentOrange => ("Orange", "Orange"),
    AccentFuchsia => ("Fuchsia", "Fuchsia"),
    AccentCyan => ("Cyan", "Cyan"),

    // ---- statistics overlay ----------------------------------------------------------------------------
    StatState => ("État", "State"),
    StatPosition => ("Position", "Position"),
    StatVideo => ("Vidéo", "Video"),
    StatDecoder => ("Décodeur", "Decoder"),
    StatHardware => ("matériel", "hardware"),
    StatSoftware => ("logiciel", "software"),
    StatDeinterlacer => ("Désentrelacement", "Deinterlacing"),
    StatFrames => ("Images", "Frames"),
    StatShown => ("affichées", "shown"),
    StatDropped => ("perdues", "dropped"),
    StatQueue => ("File d'images", "Frame queue"),
    StatAvSync => ("Synchro A/V", "A/V sync"),
    StatAudio => ("Audio", "Audio"),
    StatVirtual => ("sortie virtuelle", "virtual output"),
    StatUnderruns => ("coupures", "underruns"),
    StatRead => ("Lu", "Read"),
    StatBitrate => ("Débit", "Bitrate"),
    StatGpu => ("GPU", "GPU"),
    StatFrameTime => ("Temps d'image", "Frame time"),

    // ---- equaliser --------------------------------------------------------------------------------------
    EqEnable => ("Activer l'égaliseur", "Enable the equaliser"),
    EqPreamp => ("Préampli", "Preamp"),
    EqAutoPreamp => ("Préampli auto", "Auto preamp"),
    EqCustom => ("Personnalisé", "Custom"),
    EqHint => ("Les changements s'appliquent en direct.", "Changes apply live."),
    EqOnHint => ("Activé", "Enabled"),
    EqOffHint => ("Désactivé", "Disabled"),

    // ---- media information ---------------------------------------------------------------------------------
    InfoNoMedia => ("Aucun média ouvert", "No media open"),
    InfoGeneral => ("Général", "General"),
    InfoFile => ("Fichier", "File"),
    InfoLocation => ("Emplacement", "Location"),
    InfoContainer => ("Conteneur", "Container"),
    InfoDuration => ("Durée", "Duration"),
    InfoSize => ("Taille", "Size"),
    InfoBitrate => ("Débit", "Bitrate"),
    InfoSeekable => ("Navigation temporelle", "Seekable"),
    InfoVideo => ("Vidéo", "Video"),
    InfoVideoN => ("Vidéo {0}", "Video {0}"),
    InfoCodec => ("Codec", "Codec"),
    InfoResolution => ("Résolution", "Resolution"),
    InfoFrameRate => ("Images par seconde", "Frame rate"),
    InfoPixelFormat => ("Format de pixel", "Pixel format"),
    InfoDynamicRange => ("Plage dynamique", "Dynamic range"),
    InfoColorSpace => ("Espace colorimétrique", "Colour space"),
    InfoRotation => ("Rotation", "Rotation"),
    InfoType => ("Type", "Type"),
    InfoCoverArt => ("Pochette", "Cover art"),
    InfoLanguage => ("Langue", "Language"),
    InfoTitle => ("Titre", "Title"),
    InfoAudio => ("Audio", "Audio"),
    InfoAudioN => ("Audio {0}", "Audio {0}"),
    InfoChannels => ("Canaux", "Channels"),
    InfoSampleRate => ("Fréquence d'échantillonnage", "Sample rate"),
    InfoDefault => ("Par défaut", "Default"),
    InfoSubtitleN => ("Sous-titres {0}", "Subtitles {0}"),
    InfoText => ("Texte", "Text"),
    InfoBitmap => ("Image", "Bitmap"),
    InfoForced => ("Forcés", "Forced"),
    InfoChapters => ("Chapitres", "Chapters"),
    InfoMetadata => ("Métadonnées", "Metadata"),

    // ---- settings window -------------------------------------------------------------------------------------
    Advanced => ("Avancé", "Advanced"),
    About => ("À propos", "About"),
    SectionLook => ("Langue, thème et couleur", "Language, theme and colour"),
    SectionInterface => ("Interface", "Interface"),
    SectionBehaviour => ("Comportement", "Behaviour"),
    SectionSeeking => ("Déplacement et volume", "Seeking and volume"),
    SectionDecoding => ("Décodage", "Decoding"),
    SectionOutput => ("Sortie", "Output"),
    SectionSound => ("Son", "Sound"),
    SectionFiles => ("Fichiers", "Files"),
    SectionData => ("Données", "Data"),
    SectionGraphics => ("Carte graphique", "Graphics"),
    SetLangRow => ("Langue", "Language"),
    ThemeRow => ("Thème", "Theme"),
    AccentRow => ("Couleur d'accent", "Accent colour"),
    UiScale => ("Taille de l'interface", "Interface size"),
    UiScaleHint => ("Agrandit ou réduit tous les éléments", "Scales every element up or down"),
    ControlsTimeout => ("Masquer les commandes après", "Hide the controls after"),
    AmbientHint => ("Un halo coloré dans les bandes noires", "A soft glow in the black bars"),
    CustomTitlebar => ("Barre de titre personnalisée", "Custom title bar"),
    RestartNeeded => ("Prend effet au prochain démarrage", "Takes effect on the next launch"),
    ResumePlayback => ("Reprendre la lecture", "Resume playback"),
    ResumeHint => ("Repart là où vous vous étiez arrêté", "Continues where you left off"),
    QueueSiblings => ("Ajouter le dossier à la liste", "Queue the folder"),
    QueueSiblingsHint => ("Ouvrir un fichier met en file ceux du même dossier", "Opening one file queues the others in its folder"),
    RepeatNone => ("Aucune", "None"),
    RepeatListShort => ("La liste", "The list"),
    RepeatItemShort => ("Le titre", "The item"),
    SeekStepShort => ("Saut court (←)", "Short jump (←)"),
    SeekStepMedium => ("Saut moyen (Maj + ←)", "Medium jump (Shift + ←)"),
    SeekStepLong => ("Saut long (Ctrl + ←)", "Long jump (Ctrl + ←)"),
    VolumeStep => ("Pas du volume", "Volume step"),
    Thumbnails => ("Aperçus sur la barre de progression", "Seek-bar previews"),
    ThumbnailsHint => ("Générés en arrière-plan pour les vidéos locales", "Generated in the background for local videos"),
    HwDecode => ("Décodage matériel", "Hardware decoding"),
    HwDecodeHint => ("Utilise le décodeur de la carte graphique quand il existe", "Uses the graphics card's decoder when there is one"),
    HwDecodeHintSoftware => ("Aucun GPU sur cette machine : le décodage reste logiciel", "No GPU on this machine: decoding stays in software"),
    Deinterlace => ("Désentrelacement", "Deinterlacing"),
    DeinterlaceValue => ("Désentrelacement : {0}", "Deinterlacing: {0}"),
    DeinterlaceHint => ("Supprime l'effet de « peigne » (vieux enregistrements télé, DVD)", "Removes the \"comb\" effect (old TV recordings, DVDs)"),
    DeinterlaceOff => ("Désactivé", "Off"),
    DeinterlaceOn => ("Toujours", "Always"),
    PreventSleep => ("Garder l'écran allumé", "Keep the screen on"),
    NightMode => ("Mode nuit", "Night mode"),
    NightModeHint => (
        "Réduit les écarts de volume : explosions moins fortes, voix plus faciles à comprendre, idéal à faible volume",
        "Evens out the volume: quieter explosions, voices easier to follow, ideal at low volume"
    ),
    NightModeOn => ("Mode nuit activé", "Night mode on"),
    NightModeOff => ("Mode nuit désactivé", "Night mode off"),
    SectionDesktop => ("Bureau", "Desktop"),
    SingleInstance => ("Une seule fenêtre", "Single window"),
    SingleInstanceHint => (
        "Ouvrir un fichier alors que 400_Player tourne l'envoie à la fenêtre déjà ouverte (au prochain lancement)",
        "Opening a file while 400_Player runs sends it to the window that is already open (from the next launch)"
    ),
    PreventSleepHint => ("Pendant la lecture, l'écran ne se met pas en veille", "The screen does not go to sleep while playing"),
    HwDecodeOff => ("Logiciel", "Software"),
    HwDecodeFailed => (
        "Le décodage matériel ne fonctionne pas avec cette vidéo : lecture en mode logiciel",
        "Hardware decoding does not work with this video: playing in software"
    ),
    OutputDevice => ("Sortie audio", "Audio output"),
    DeviceDefault => ("Périphérique par défaut", "Default device"),
    VirtualAudioNote => ("Aucune carte son : la lecture est silencieuse", "No sound card: playback is silent"),
    PreferredAudio => ("Langues audio préférées", "Preferred audio languages"),
    PreferredHint => ("Codes de langue séparés par des virgules", "Language codes separated by commas"),
    PreferredSubs => ("Langues de sous-titres préférées", "Preferred subtitle languages"),
    AutoSubtitles => ("Charger les sous-titres automatiquement", "Load subtitles automatically"),
    AutoSubtitlesHint => ("Fichiers .srt, .vtt ou .ass portant le nom de la vidéo", "Files named like the video (.srt, .vtt, .ass)"),
    Style => ("Style", "Style"),
    SubPreview => ("Ceci est un aperçu des sous-titres.", "This is a preview of the subtitles."),
    SubSize => ("Taille", "Size"),
    SubColor => ("Couleur", "Colour"),
    SubBold => ("Gras", "Bold"),
    SubOutline => ("Contour", "Outline"),
    SubBackground => ("Fond", "Background"),
    SubBackgroundOpacity => ("Opacité du fond", "Background opacity"),
    SubPosition => ("Hauteur", "Vertical position"),
    ShowFolder => ("Afficher le dossier", "Show the folder"),
    ConfigFolder => ("Dossier de configuration", "Configuration folder"),
    PlaybackHistory => ("Historique de lecture", "Playback history"),
    HistoryCleared => ("Historique effacé", "History cleared"),
    ResetSettings => ("Réinitialiser les réglages", "Reset the settings"),
    ResetHint => ("Rétablit les valeurs d'origine", "Restores the defaults"),
    ResetConfirm => ("Cliquez encore pour confirmer", "Click again to confirm"),
    SettingsReset => ("Réglages réinitialisés", "Settings reset"),
    EphemeralNote => ("Mode éphémère : rien n'est enregistré dans votre profil.", "Ephemeral mode: nothing is saved to your profile."),
    GpuAdapter => ("Adaptateur", "Adapter"),
    GpuBackend => ("API graphique", "Graphics API"),
    GpuDriver => ("Pilote", "Driver"),
    Gpu16Bit => ("Textures 16 bits", "16-bit textures"),
    GpuSoftware => ("rendu logiciel", "software rendering"),
    AboutVersion => ("Version", "Version"),
    AboutLicense => ("Licence", "License"),
    AboutEngine => ("Moteur A/V", "A/V engine"),
    AboutPlatform => ("Plateforme", "Platform"),
    AboutCredits => ("Crédits", "Credits"),
    AboutIndependent => (
        "400_Player est une réécriture indépendante en Rust, inspirée de VLC. Il ne contient aucun code de VLC.",
        "400_Player is an independent rewrite in Rust, inspired by VLC. It contains no VLC code."
    ),
    AboutBuiltWith => ("Construit avec", "Built with"),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extracts the `{n}` placeholders of a string, sorted.
    fn placeholders(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            if let Some(j) = rest[i..].find('}') {
                out.push(rest[i..i + j + 1].to_owned());
                rest = &rest[i + j + 1..];
            } else {
                break;
            }
        }
        out.sort();
        out
    }

    #[test]
    fn both_languages_use_the_same_placeholders() {
        for &k in Key::ALL {
            assert_eq!(placeholders(k.raw_fr()), placeholders(k.raw_en()), "{k:?}");
        }
    }

    #[test]
    fn nothing_is_empty_or_padded() {
        for &k in Key::ALL {
            for text in [k.raw_fr(), k.raw_en()] {
                assert!(!text.is_empty(), "{k:?} is empty");
                assert_eq!(text, text.trim(), "{k:?} has leading or trailing spaces");
                assert!(!text.contains("  "), "{k:?} has a double space");
            }
        }
    }

    #[test]
    fn french_typography_rules() {
        assert_eq!(french_typography("Volume : 80 %"), "Volume\u{a0}: 80\u{a0}%");
        assert_eq!(french_typography("Quoi ?"), "Quoi\u{a0}?");
        assert_eq!(french_typography("« bonjour »"), "«\u{a0}bonjour\u{a0}»");
        assert_eq!(french_typography("Sans changement"), "Sans changement");
        assert_eq!(french_typography("a b"), "a b");
    }

    #[test]
    fn lookup_and_formatting() {
        let _guard = test_lock();
        set_language(Language::French);
        assert_eq!(current(), Language::French);
        assert_eq!(t(Key::Pause), "Pause");
        assert_eq!(tf(Key::PlaylistTotal, &[&12, &"1 h"]), "12 éléments · 1 h");
        assert_eq!(t1(Key::VolumeValue, 80), "Volume\u{a0}: 80\u{a0}%");
        set_language(Language::English);
        assert_eq!(current(), Language::English);
        assert_eq!(t(Key::Play), "Play");
        assert_eq!(t_in(Key::Play, Language::French), "Lecture");
        assert_eq!(t1(Key::VolumeValue, 80), "Volume: 80 %");
    }

    #[test]
    fn keys_are_dense() {
        for (i, &k) in Key::ALL.iter().enumerate() {
            assert_eq!(k as usize, i, "{k:?} must sit at its own index (the French table relies on it)");
        }
    }
}
