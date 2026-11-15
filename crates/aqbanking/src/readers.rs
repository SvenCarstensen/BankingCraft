//! OS reader discovery only. Card protocols/PIN handling are in Libchipcard.
use banking::{Error, Result};
use pcsc::{Context, ReaderState, Scope, State};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ReaderInfo {
    pub name: String,
    pub card_present: bool,
    pub cyberjack: bool,
}

pub fn is_cyberjack(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("cyberjack") || n.contains("reiner sct") || n.contains("reiner-sct")
}

pub fn list_readers() -> Result<Vec<ReaderInfo>> {
    let ctx = Context::establish(Scope::User).map_err(|e| Error::Other(format!("PC/SC: {e}")))?;
    let names = match ctx.list_readers_owned() {
        Ok(n) => n,
        Err(pcsc::Error::NoReadersAvailable) => return Ok(vec![]),
        Err(e) => return Err(Error::Other(e.to_string())),
    };
    let mut states: Vec<ReaderState> = names.iter().map(|n| ReaderState::new(n.clone(), State::UNAWARE)).collect();
    ctx.get_status_change(Duration::from_millis(200), &mut states).ok();
    let mut out: Vec<ReaderInfo> = names
        .iter()
        .zip(states.iter())
        .map(|(n, s)| {
            let name = n.to_string_lossy().to_string();
            ReaderInfo {
                cyberjack: is_cyberjack(&name),
                card_present: s.event_state().contains(State::PRESENT),
                name,
            }
        })
        .collect();
    // cyberJack first, then readers with a card
    out.sort_by_key(|r| (!r.cyberjack, !r.card_present));
    Ok(out)
}
