use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use serde::{Deserialize, Serialize};

use crate::domain::OtherOpenBetRow;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManualPositionEntry {
    pub event: String,
    pub market: String,
    pub selection: String,
    pub venue: String,
    pub side: String,
    pub odds: f64,
    pub stake: f64,
    pub current_cashout_value: Option<f64>,
    pub status: String,
}

impl Default for ManualPositionEntry {
    fn default() -> Self {
        Self {
            event: String::new(),
            market: String::new(),
            selection: String::new(),
            venue: String::new(),
            side: String::from("back"),
            odds: 0.0,
            stake: 0.0,
            current_cashout_value: None,
            status: String::from("manual"),
        }
    }
}

impl ManualPositionEntry {
    pub fn display_key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            normalize_key(&self.event),
            normalize_key(&self.market),
            normalize_key(&self.selection),
            normalize_key(&self.venue)
        )
    }

    pub fn to_other_open_bet(&self) -> OtherOpenBetRow {
        OtherOpenBetRow {
            venue: self.venue.clone(),
            event: self.event.clone(),
            label: self.selection.clone(),
            market: self.market.clone(),
            side: self.side.clone(),
            odds: self.odds,
            stake: self.stake,
            status: self.status.clone(),
            funding_kind: String::from("manual"),
            current_cashout_value: self.current_cashout_value,
            supports_cash_out: self.current_cashout_value.is_some(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualPositionField {
    Event,
    Market,
    Selection,
    Venue,
    Odds,
    Stake,
    Cashout,
    Save,
}

impl ManualPositionField {
    pub const ALL: [Self; 8] = [
        Self::Event,
        Self::Market,
        Self::Selection,
        Self::Venue,
        Self::Odds,
        Self::Stake,
        Self::Cashout,
        Self::Save,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Event => "Event",
            Self::Market => "Market",
            Self::Selection => "Selection",
            Self::Venue => "Venue",
            Self::Odds => "Odds",
            Self::Stake => "Stake",
            Self::Cashout => "Cashout",
            Self::Save => "Save",
        }
    }

    pub fn editable(self) -> bool {
        !matches!(self, Self::Save)
    }
}

#[derive(Debug, Clone)]
pub struct ManualPositionOverlayState {
    pub draft: ManualPositionEntry,
    pub selected_field: ManualPositionField,
    pub editing: bool,
    pub input_buffer: String,
}

impl ManualPositionOverlayState {
    pub fn new(draft: ManualPositionEntry) -> Self {
        Self {
            draft,
            selected_field: ManualPositionField::Event,
            editing: false,
            input_buffer: String::new(),
        }
    }

    pub fn selected_field(&self) -> ManualPositionField {
        self.selected_field
    }

    pub fn select_next_field(&mut self) {
        let index = ManualPositionField::ALL
            .iter()
            .position(|field| *field == self.selected_field)
            .unwrap_or(0);
        self.selected_field =
            ManualPositionField::ALL[(index + 1) % ManualPositionField::ALL.len()];
    }

    pub fn select_previous_field(&mut self) {
        let index = ManualPositionField::ALL
            .iter()
            .position(|field| *field == self.selected_field)
            .unwrap_or(0);
        self.selected_field = if index == 0 {
            ManualPositionField::ALL[ManualPositionField::ALL.len() - 1]
        } else {
            ManualPositionField::ALL[index - 1]
        };
    }

    pub fn begin_edit(&mut self) {
        if !self.selected_field.editable() {
            return;
        }
        self.editing = true;
        self.input_buffer = self.selected_value();
    }

    pub fn cancel_edit(&mut self) {
        self.editing = false;
        self.input_buffer.clear();
    }

    pub fn push_char(&mut self, character: char) {
        self.input_buffer.push(character);
    }

    pub fn backspace(&mut self) {
        self.input_buffer.pop();
    }

    pub fn selected_value(&self) -> String {
        self.selected_value_for(self.selected_field)
    }

    pub fn selected_value_for(&self, field: ManualPositionField) -> String {
        match field {
            ManualPositionField::Event => self.draft.event.clone(),
            ManualPositionField::Market => self.draft.market.clone(),
            ManualPositionField::Selection => self.draft.selection.clone(),
            ManualPositionField::Venue => self.draft.venue.clone(),
            ManualPositionField::Odds => format_decimal(self.draft.odds),
            ManualPositionField::Stake => format_decimal(self.draft.stake),
            ManualPositionField::Cashout => self
                .draft
                .current_cashout_value
                .map(format_decimal)
                .unwrap_or_default(),
            ManualPositionField::Save => String::from("Press Enter to save"),
        }
    }

    pub fn apply_edit(&mut self) -> Result<()> {
        let value = self.input_buffer.trim();
        match self.selected_field {
            ManualPositionField::Event => self.draft.event = value.to_string(),
            ManualPositionField::Market => self.draft.market = value.to_string(),
            ManualPositionField::Selection => self.draft.selection = value.to_string(),
            ManualPositionField::Venue => self.draft.venue = value.to_string(),
            ManualPositionField::Odds => self.draft.odds = parse_decimal(value)?,
            ManualPositionField::Stake => self.draft.stake = parse_decimal(value)?,
            ManualPositionField::Cashout => {
                self.draft.current_cashout_value = if value.is_empty() {
                    None
                } else {
                    Some(parse_decimal(value)?)
                };
            }
            ManualPositionField::Save => {}
        }
        self.cancel_edit();
        Ok(())
    }
}

pub fn default_config_path() -> PathBuf {
    if let Some(path) = env::var_os("SABI_MANUAL_POSITIONS_PATH") {
        return PathBuf::from(path);
    }
    let config_root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    config_root.join("sabi").join("manual_positions.json")
}

pub fn load_entries_or_default(path: &Path) -> Result<(Vec<ManualPositionEntry>, String)> {
    if !path.exists() {
        return Ok((
            Vec::new(),
            String::from("Using empty manual positions config."),
        ));
    }
    let content = fs::read_to_string(path)?;
    let mut entries = serde_json::from_str::<Vec<ManualPositionEntry>>(&content)?;
    let original_len = entries.len();
    entries.retain(|entry| !is_example_entry(entry));
    let note = if entries.len() == original_len {
        format!("Loaded manual positions from {}.", path.display())
    } else if entries.is_empty() {
        format!(
            "Ignored example manual positions in {}; no live manual positions loaded.",
            path.display()
        )
    } else {
        format!(
            "Loaded manual positions from {} after filtering {} example entr{}.",
            path.display(),
            original_len - entries.len(),
            if original_len - entries.len() == 1 {
                "y"
            } else {
                "ies"
            }
        )
    };
    Ok((entries, note))
}

pub fn save_entries(path: &Path, entries: &[ManualPositionEntry]) -> Result<String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(entries)? + "\n")?;
    Ok(format!("Saved manual positions to {}.", path.display()))
}

fn parse_decimal(value: &str) -> Result<f64> {
    Ok(value.parse::<f64>()?)
}

fn format_decimal(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn normalize_key(value: &str) -> String {
    value
        .to_lowercase()
        .replace("vs", "v")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_example_entry(entry: &ManualPositionEntry) -> bool {
    normalize_key(&entry.event) == "malta v luxembourg"
        && normalize_key(&entry.market) == "full time result"
        && normalize_key(&entry.selection) == "draw"
        && normalize_key(&entry.venue) == "betway"
        && entry.side.eq_ignore_ascii_case("back")
        && (entry.odds - 3.2).abs() < f64::EPSILON
        && (entry.stake - 50.0).abs() < f64::EPSILON
        && entry.current_cashout_value.is_none()
        && entry.status.eq_ignore_ascii_case("manual")
}

#[cfg(test)]
mod tests {
    use super::{
        load_entries_or_default, save_entries, ManualPositionEntry, ManualPositionField,
        ManualPositionOverlayState,
    };

    #[test]
    fn save_and_load_round_trip() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let path = temp_dir.path().join("manual_positions.json");
        let entries = vec![ManualPositionEntry {
            event: String::from("Malta v Luxembourg"),
            market: String::from("Match Betting"),
            selection: String::from("X"),
            venue: String::from("betway"),
            odds: 3.2,
            stake: 50.0,
            ..ManualPositionEntry::default()
        }];

        save_entries(&path, &entries).expect("save");
        let (loaded, note) = load_entries_or_default(&path).expect("load");

        assert_eq!(loaded, entries);
        assert!(note.contains("Loaded manual positions"));
    }

    #[test]
    fn ignores_seed_example_manual_position() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let path = temp_dir.path().join("manual_positions.json");
        let entries = vec![ManualPositionEntry {
            event: String::from("Malta vs Luxembourg"),
            market: String::from("Full-time result"),
            selection: String::from("Draw"),
            venue: String::from("betway"),
            odds: 3.2,
            stake: 50.0,
            ..ManualPositionEntry::default()
        }];

        save_entries(&path, &entries).expect("save");
        let (loaded, note) = load_entries_or_default(&path).expect("load");

        assert!(loaded.is_empty());
        assert!(note.contains("Ignored example manual positions"));
    }

    #[test]
    fn manual_overlay_starts_on_event_with_blank_venue() {
        let overlay = ManualPositionOverlayState::new(ManualPositionEntry::default());

        assert_eq!(overlay.selected_field(), ManualPositionField::Event);
        assert!(overlay.draft.venue.is_empty());
    }
}
