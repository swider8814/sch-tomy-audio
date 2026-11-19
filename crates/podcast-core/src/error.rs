//! Error type shared by the processing core.
//!
//! Every error carries a friendly Polish message for non-technical users
//! (`user_message`) and technical details for the expandable log (`details`).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// The user cancelled the operation.
    Cancelled,
    /// ffmpeg or ffprobe could not be started.
    ToolMissing,
    /// The input file cannot be read or decoded.
    InputUnreadable,
    /// The input has no audio stream.
    NoAudio,
    /// The input decodes but is (almost) completely silent.
    InputSilent,
    IntroUnreadable,
    OutroUnreadable,
    CoverUnreadable,
    /// The output folder cannot be written to.
    OutputNotWritable,
    /// Start/end markers make no sense for this file.
    InvalidCut,
    /// ffmpeg failed in a processing step.
    Processing,
    /// A teaser pattern could not be stored or loaded.
    Pattern,
    /// Settings could not be read or written.
    Settings,
}

#[derive(Debug, Clone)]
pub struct CoreError {
    pub kind: ErrorKind,
    pub details: String,
}

pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    pub fn new(kind: ErrorKind, details: impl Into<String>) -> Self {
        Self {
            kind,
            details: details.into(),
        }
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "cancelled by user")
    }

    pub fn is_cancelled(&self) -> bool {
        self.kind == ErrorKind::Cancelled
    }

    /// Re-labels a generic input error as an intro/outro/cover problem.
    pub fn with_kind(mut self, kind: ErrorKind) -> Self {
        if self.kind != ErrorKind::Cancelled && self.kind != ErrorKind::ToolMissing {
            self.kind = kind;
        }
        self
    }

    /// Message shown to the user (Polish, "Ty" form, no technical jargon).
    pub fn user_message(&self) -> String {
        match self.kind {
            ErrorKind::Cancelled => "Anulowano.".into(),
            ErrorKind::ToolMissing => "Nie udało się uruchomić narzędzia do obróbki dźwięku (ffmpeg). Zainstaluj aplikację ponownie.".into(),
            ErrorKind::InputUnreadable => "Nie udało się odczytać pliku. Sprawdź, czy to nagranie audio albo wideo.".into(),
            ErrorKind::NoAudio => "Ten plik nie zawiera dźwięku. Sprawdź, czy to właściwe nagranie.".into(),
            ErrorKind::InputSilent => "Nagranie jest ciche albo puste (po przycięciu nie zostało nic do wyrównania głośności). Sprawdź znaczniki start i koniec.".into(),
            ErrorKind::IntroUnreadable => "Nie udało się odczytać pliku intro. Wybierz go ponownie w Ustawieniach.".into(),
            ErrorKind::OutroUnreadable => "Nie udało się odczytać pliku outro. Wybierz go ponownie w Ustawieniach.".into(),
            ErrorKind::CoverUnreadable => "Nie udało się odczytać okładki. Wybierz plik JPG ponownie w Ustawieniach.".into(),
            ErrorKind::OutputNotWritable => "Nie udało się zapisać wyniku. Sprawdź, czy folder wyjściowy istnieje i czy jest na nim wolne miejsce.".into(),
            ErrorKind::InvalidCut => "Znaczniki start i koniec są nieprawidłowe (start musi być przed końcem i w obrębie nagrania). Popraw je i spróbuj ponownie.".into(),
            ErrorKind::Processing => "Obróbka nagrania nie powiodła się. Szczegóły znajdziesz w logu.".into(),
            ErrorKind::Pattern => "Nie udało się zapisać albo odczytać wzorca zajawki.".into(),
            ErrorKind::Settings => "Nie udało się zapisać ustawień.".into(),
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.details)
    }
}

impl std::error::Error for CoreError {}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::new(ErrorKind::OutputNotWritable, e.to_string())
    }
}
