use std::time::Duration;

use bevy::platform::collections::HashMap;
use bevy_replicon::prelude::ClientId;

/// How much of something each client may still do straight away: a burst, used up a time at a
/// go and refilled at a steady rate, so a client can't flood the server or everyone else.
pub(super) struct Allowances {
    burst: f32,
    per_second: f32,
    clients: HashMap<ClientId, Allowance>,
}

struct Allowance {
    left: f32,
    at: Duration,
}

impl Allowances {
    pub(super) fn new(burst: f32, per_second: f32) -> Self {
        Self {
            burst,
            per_second,
            clients: HashMap::new(),
        }
    }

    /// Takes one from the client's allowance at `now`, if it has one left.
    pub(super) fn take(&mut self, client: ClientId, now: Duration) -> bool {
        let allowance = self.clients.entry(client).or_insert(Allowance {
            left: self.burst,
            at: now,
        });
        let refilled = (now - allowance.at).as_secs_f32() * self.per_second;
        allowance.left = (allowance.left + refilled).min(self.burst);
        allowance.at = now;
        let allowed = allowance.left >= 1.0;
        if allowed {
            allowance.left -= 1.0;
        }
        allowed
    }

    /// Forgets a client that's left.
    pub(super) fn forget(&mut self, client: ClientId) {
        self.clients.remove(&client);
    }
}
