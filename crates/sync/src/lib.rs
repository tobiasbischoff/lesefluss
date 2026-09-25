//! Koordination der Feedly-Zyklen pro Konto.
//!
//! Erst-Sync, Delta-Sync, Outbox-Versand und serverweite Aktionen laufen über
//! denselben Coordinator: höchstens ein Auftrag gleichzeitig, weitere Wünsche
//! werden mit ihrer Priorität vorgemerkt. Auth- und Quotenpausen gelten für
//! alle Wege, auch für den manuellen Refresh.

use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Job {
    /// Periodischer Hintergrundlauf.
    Scheduled,
    /// Delta-Sync nach Benutzerwunsch.
    Refresh,
    /// Versand der wartenden Outbox-Zeilen.
    Outbox,
    /// Erst-Sync nach dem Verbinden.
    Initial,
    /// Große serverseitige Aktion, z. B. „alles gelesen“.
    ServerAction,
}

impl Job {
    /// Höhere Zahl gewinnt, wenn mehrere Wünsche anstehen.
    pub fn priority(self) -> u8 {
        match self {
            Job::Scheduled => 0,
            Job::Outbox => 1,
            Job::Refresh => 2,
            Job::ServerAction => 3,
            Job::Initial => 4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pause {
    /// Authentifizierung nötig; alle Läufe warten auf neue Anmeldung.
    Auth,
    /// Drosselung bis zu diesem Zeitpunkt (ms).
    Quota(i64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// Jetzt starten.
    Start,
    /// Läuft bereits; der Wunsch ist vorgemerkt.
    Queued,
    /// Pause aktiv; der Wunsch wird verworfen und erst nach erneutem Anfordern
    /// erneut betrachtet.
    Paused(Pause),
    /// Gesperrt (z. B. abgemeldet).
    Blocked,
}

#[derive(Debug, Default)]
struct AccountState {
    running: Option<Job>,
    queued: Option<Job>,
    pause: Option<Pause>,
    blocked: bool,
}

#[derive(Debug, Default)]
pub struct SyncCoordinator {
    accounts: HashMap<String, AccountState>,
    now_ms: i64,
}

/// Ergebnis eines abgeschlossenen Laufs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub finished: Option<Job>,
    pub started_next: Option<Job>,
}

impl SyncCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_now(&mut self, now_ms: i64) {
        self.now_ms = now_ms;
    }

    fn state(&mut self, account: &str) -> &mut AccountState {
        self.accounts.entry(account.to_string()).or_default()
    }

    fn pause_active(state: &AccountState, now: i64) -> Option<Pause> {
        match state.pause {
            Some(Pause::Auth) => Some(Pause::Auth),
            Some(Pause::Quota(until)) if until > now => Some(Pause::Quota(until)),
            _ => None,
        }
    }

    /// Fordert einen Lauf an. Liegt bereits einer vor, wird der wichtigere
    /// Wunsch vorgemerkt; ein Refresh erhöht nur die Priorität.
    pub fn request(&mut self, account: &str, job: Job) -> Decision {
        let now = self.now_ms;
        let state = self.state(account);
        if state.blocked {
            return Decision::Blocked;
        }
        if let Some(pause) = Self::pause_active(state, now) {
            return Decision::Paused(pause);
        }
        if state.running.is_some() {
            state.queued = match state.queued {
                Some(pending) if pending.priority() >= job.priority() => Some(pending),
                _ => Some(job),
            };
            return Decision::Queued;
        }
        state.running = Some(job);
        Decision::Start
    }

    /// Meldet das Ende eines Laufs und liefert den nächsten Start sofort mit.
    pub fn finish(&mut self, account: &str, now_ms: i64) -> Outcome {
        self.now_ms = now_ms;
        let state = self.state(account);
        state.running = None;
        if Self::pause_active(state, now_ms).is_some() {
            state.queued = None;
            return Outcome {
                finished: None,
                started_next: None,
            };
        }
        match state.queued.take() {
            Some(next) => {
                state.running = Some(next);
                Outcome {
                    finished: None,
                    started_next: Some(next),
                }
            }
            None => Outcome {
                finished: None,
                started_next: None,
            },
        }
    }

    pub fn pause(&mut self, account: &str, pause: Pause) {
        let state = self.state(account);
        state.pause = Some(pause);
        state.queued = None;
    }

    /// Hebt eine Pause auf (neue Anmeldung, Zeit abgelaufen).
    pub fn resume(&mut self, account: &str) {
        let state = self.state(account);
        state.pause = None;
    }

    /// Abmelden: alles wird verworfen, bis `resume` nach neuem Verbinden.
    pub fn block(&mut self, account: &str) {
        let state = self.state(account);
        state.blocked = true;
        state.running = None;
        state.queued = None;
        state.pause = None;
    }

    pub fn unblock(&mut self, account: &str) {
        self.state(account).blocked = false;
    }

    pub fn running(&self, account: &str) -> Option<Job> {
        self.accounts.get(account).and_then(|s| s.running)
    }

    pub fn queued(&self, account: &str) -> Option<Job> {
        self.accounts.get(account).and_then(|s| s.queued)
    }

    pub fn pause_state(&self, account: &str) -> Option<Pause> {
        self.accounts.get(account).and_then(|s| s.pause)
    }

    pub fn is_blocked(&self, account: &str) -> bool {
        self.accounts
            .get(account)
            .map(|s| s.blocked)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laufende_zyklen_werden_nicht_verdoppelt() {
        let mut c = SyncCoordinator::new();
        assert_eq!(c.request("feedly-1", Job::Initial), Decision::Start);
        assert_eq!(c.request("feedly-1", Job::Scheduled), Decision::Queued);
        assert_eq!(c.running("feedly-1"), Some(Job::Initial));
    }

    #[test]
    fn der_wichtigere_wunsch_gewinnt() {
        let mut c = SyncCoordinator::new();
        c.request("feedly-1", Job::Initial);
        c.request("feedly-1", Job::Scheduled);
        c.request("feedly-1", Job::ServerAction);
        assert_eq!(c.queued("feedly-1"), Some(Job::ServerAction));
        let out = c.finish("feedly-1", 0);
        assert_eq!(out.started_next, Some(Job::ServerAction));
    }

    #[test]
    fn niedrigere_prioritaet_ersetzt_den_wunsch_nicht() {
        let mut c = SyncCoordinator::new();
        c.request("feedly-1", Job::Initial);
        c.request("feedly-1", Job::Refresh);
        c.request("feedly-1", Job::Scheduled);
        assert_eq!(c.queued("feedly-1"), Some(Job::Refresh));
    }

    #[test]
    fn auth_pause_hält_auch_den_manuellen_refresh_an() {
        let mut c = SyncCoordinator::new();
        c.pause("feedly-1", Pause::Auth);
        assert_eq!(
            c.request("feedly-1", Job::Refresh),
            Decision::Paused(Pause::Auth)
        );
        assert_eq!(
            c.request("feedly-1", Job::Initial),
            Decision::Paused(Pause::Auth)
        );
        c.resume("feedly-1");
        assert_eq!(c.request("feedly-1", Job::Refresh), Decision::Start);
    }

    #[test]
    fn quotenpause_gilt_nur_bis_zum_zeitpunkt() {
        let mut c = SyncCoordinator::new();
        c.set_now(1_000);
        c.pause("feedly-1", Pause::Quota(2_000));
        assert_eq!(
            c.request("feedly-1", Job::Refresh),
            Decision::Paused(Pause::Quota(2_000))
        );
        c.set_now(3_000);
        assert_eq!(c.request("feedly-1", Job::Refresh), Decision::Start);
    }

    #[test]
    fn pause_verwirft_bereits_vorgemerkte_arbeit() {
        let mut c = SyncCoordinator::new();
        c.request("feedly-1", Job::Initial);
        c.request("feedly-1", Job::Refresh);
        c.pause("feedly-1", Pause::Auth);
        let out = c.finish("feedly-1", 0);
        assert_eq!(
            out.started_next, None,
            "keine Folgearbeit während der Pause"
        );
        assert_eq!(c.queued("feedly-1"), None);
    }

    #[test]
    fn abmelden_sperrt_bis_zur_erneuten_verbindung() {
        let mut c = SyncCoordinator::new();
        c.request("feedly-1", Job::Refresh);
        c.block("feedly-1");
        assert!(c.is_blocked("feedly-1"));
        assert_eq!(c.request("feedly-1", Job::Refresh), Decision::Blocked);
        c.unblock("feedly-1");
        assert_eq!(c.request("feedly-1", Job::Refresh), Decision::Start);
    }

    #[test]
    fn konten_synchronisieren_unabhaengig() {
        let mut c = SyncCoordinator::new();
        assert_eq!(c.request("konto-a", Job::Refresh), Decision::Start);
        assert_eq!(c.request("konto-b", Job::Refresh), Decision::Start);
        assert_eq!(c.running("konto-a"), Some(Job::Refresh));
        assert_eq!(c.running("konto-b"), Some(Job::Refresh));
    }
}
