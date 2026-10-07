//! "Completed item" chips. The Live Client Data API has no purchase events (Overwolf's League
//! events don't either; apps compare inventories), so the own player's items are compared
//! between player-list reads. The list is read every few seconds, and at once when the gold
//! jumps (a purchase, a sale or an undo shows there first, see [`crate::gold`]).
//!
//! Only finished items count (Data Dragon: built from components, upgrades into nothing, not a
//! consumable or trinket; see [`crate::ddragon::ItemData`]). The shop's UNDO button must not
//! leave false chips:
//! - a new item is only confirmed after it stayed [`CONFIRM_SECS`] in the inventory; gone before
//!   that = no chip (undo, or sold at once);
//! - a confirmed item that disappears while its gold comes back in full (≥ 90 % of its cost; a
//!   sale gives back 70 %), or its components reappear, was undone: the chip is withdrawn;
//! - an item that reappears after a sale with ~70 % of its cost paid back is the sale undone,
//!   not a second purchase.

use crate::ddragon::StaticData;
use cv_core::{EventKind, GameEvent};
use std::collections::HashMap;

/// Game seconds a new item must stay before it gets its chip.
pub const CONFIRM_SECS: f64 = 5.0;
/// An undo gives back the full price, a sale 70 %.
const UNDO_SHARE: f64 = 0.9;
const SALE_SHARE_MAX: f64 = 0.85;

#[derive(Debug, Clone)]
struct Pending {
    item: u32,
    /// Game time of the purchase.
    t: f64,
}

#[derive(Debug, Clone)]
struct Confirmed {
    event_id: String,
    item: u32,
}

#[derive(Debug, Default)]
pub struct ItemTracker {
    /// Item id -> count at the last read (None until the first read: the starting inventory
    /// never makes chips, e.g. when the app starts mid-game).
    last: Option<HashMap<u32, u32>>,
    last_gold: Option<(f64, f64)>,
    pending: Vec<Pending>,
    confirmed: Vec<Confirmed>,
    /// Items sold (not undone), game time: a sale can be undone too.
    sold: Vec<(u32, f64)>,
    /// Finished items bought so far (for "2nd item").
    count: u32,
}

/// What a read changed.
#[derive(Debug, Default, PartialEq)]
pub struct ItemChanges {
    pub new: Vec<GameEvent>,
    pub withdrawn: Vec<String>,
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

impl ItemTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Items waiting for their confirmation (the player list is then read more often).
    pub fn waiting(&self) -> bool {
        !self.pending.is_empty()
    }

    /// One read of the own inventory at game time `gt` with the current gold. `spent_at` = the
    /// game time the gold last dropped (the purchase moment, more exact than this read).
    pub fn observe(&mut self, gt: f64, inv: &HashMap<u32, u32>, gold: Option<f64>, spent_at: Option<f64>, data: &StaticData) -> ItemChanges {
        let mut out = ItemChanges::default();
        let Some(last) = self.last.replace(inv.clone()) else {
            self.last_gold = gold.map(|g| (gt, g));
            return out;
        };
        let prev_gold = std::mem::replace(&mut self.last_gold, gold.map(|g| (gt, g)));
        // Gold change since the last read, without the passive income (~2 per second).
        let gold_back = match (prev_gold, gold) {
            (Some((t0, g0)), Some(g1)) => Some(g1 - g0 - 2.5 * (gt - t0).max(0.0)),
            _ => None,
        };
        let finished = |id: u32| data.item(id).is_some_and(|i| i.finished);
        let mut added: Vec<u32> = Vec::new();
        let mut removed: Vec<u32> = Vec::new();
        for (&id, &n) in inv {
            let before = last.get(&id).copied().unwrap_or(0);
            added.extend(std::iter::repeat_n(id, n.saturating_sub(before) as usize));
        }
        for (&id, &n) in &last {
            let now = inv.get(&id).copied().unwrap_or(0);
            removed.extend(std::iter::repeat_n(id, n.saturating_sub(now) as usize));
        }

        for &id in removed.iter().filter(|&&id| finished(id)) {
            let total = data.item(id).map(|i| i.total as f64).unwrap_or(0.0);
            if let Some(i) = self.pending.iter().rposition(|p| p.item == id) {
                // Gone before it was confirmed: undone (or sold at once). No chip.
                self.pending.remove(i);
                log::info!("items: {id} gone before it was confirmed (undo or sold): no chip");
                continue;
            }
            let components_back = data.item(id).is_some_and(|i| i.from.iter().any(|c| added.contains(c)));
            let full_refund = gold_back.is_some_and(|g| total > 0.0 && g >= UNDO_SHARE * total);
            if let Some(i) = self.confirmed.iter().rposition(|c| c.item == id) {
                if components_back || full_refund {
                    let c = self.confirmed.remove(i);
                    self.count = self.count.saturating_sub(1);
                    log::info!("items: {id} undone (gold back {:?}, components back {components_back}): chip withdrawn", gold_back.map(|g| g.round()));
                    out.withdrawn.push(c.event_id);
                    continue;
                }
            }
            self.sold.push((id, gt));
        }

        for &id in added.iter().filter(|&&id| finished(id)) {
            let total = data.item(id).map(|i| i.total as f64).unwrap_or(0.0);
            // A sale undone: the item comes back for its sell price (~70 %), recently sold.
            if let Some(i) = self.sold.iter().rposition(|(s, t)| *s == id && gt - t < 120.0) {
                if gold_back.is_some_and(|g| -g < SALE_SHARE_MAX * total) {
                    self.sold.remove(i);
                    log::info!("items: {id} back after a sale (sale undone): no new chip");
                    continue;
                }
            }
            let t = spent_at.filter(|s| *s <= gt + 0.01 && gt - s < 4.0).unwrap_or(gt);
            self.pending.push(Pending { item: id, t });
        }

        let mut still = Vec::new();
        for p in std::mem::take(&mut self.pending) {
            if gt - p.t < CONFIRM_SECS {
                still.push(p);
                continue;
            }
            let Some(info) = data.item(p.item) else { continue };
            self.count += 1;
            let id = format!("item-{}-{:.1}", p.item, p.t);
            let parts: Vec<String> = info.from.iter().filter_map(|c| data.item(*c).map(|i| i.name.clone())).collect();
            let ev = GameEvent::new(id.clone(), EventKind::ItemCompleted, p.t, format!("Completed {}", info.name))
                .with_icon("item", p.item.to_string(), data.version.clone())
                .fact("Cost", format!("{} gold", info.total))
                .fact("Item", format!("{} finished item", ordinal(self.count)))
                .fact("Built from", parts.join(", "));
            self.confirmed.push(Confirmed { event_id: id, item: p.item });
            out.new.push(ev);
        }
        self.pending = still;
        out
    }
}

/// The own player's inventory from a player-list entry's items: item id -> count (stacks of
/// consumables count once; they never make chips anyway).
pub fn inventory(items: impl IntoIterator<Item = u32>) -> HashMap<u32, u32> {
    let mut m = HashMap::new();
    for id in items.into_iter().filter(|&i| i > 0) {
        *m.entry(id).or_insert(0) += 1;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::parse_items;

    fn data() -> StaticData {
        StaticData { version: "16.20.1".into(), items: parse_items(include_str!("../tests/ddragon/items-16.20.json")), spells: Vec::new() }
    }

    struct Game {
        tr: ItemTracker,
        d: StaticData,
        inv: Vec<u32>,
        gold: f64,
        t: f64,
        events: Vec<GameEvent>,
    }

    impl Game {
        fn new(inv: &[u32], gold: f64) -> Game {
            let mut g = Game { tr: ItemTracker::new(), d: data(), inv: inv.to_vec(), gold, t: 60.0, events: Vec::new() };
            g.read();
            g
        }
        /// `secs` pass (passive gold 2/s), then the list is read.
        fn wait(&mut self, secs: f64) {
            self.t += secs;
            self.gold += 2.0 * secs;
            self.read();
        }
        fn read(&mut self) {
            let c = self.tr.observe(self.t, &inventory(self.inv.iter().copied()), Some(self.gold), Some(self.t), &self.d);
            self.events.retain(|e| !c.withdrawn.contains(&e.id));
            self.events.extend(c.new);
        }
        fn buy(&mut self, id: u32, uses: &[u32]) {
            for u in uses {
                let i = self.inv.iter().position(|x| x == u).unwrap();
                self.inv.remove(i);
            }
            self.inv.push(id);
            let paid: u32 = self.d.item(id).unwrap().total - uses.iter().map(|u| self.d.item(*u).unwrap().total).sum::<u32>();
            self.gold -= paid as f64;
        }
        /// UNDO: the purchase reversed exactly.
        fn undo(&mut self, id: u32, uses: &[u32]) {
            let paid: u32 = self.d.item(id).unwrap().total - uses.iter().map(|u| self.d.item(*u).unwrap().total).sum::<u32>();
            let i = self.inv.iter().position(|x| *x == id).unwrap();
            self.inv.remove(i);
            self.inv.extend(uses);
            self.gold += paid as f64;
        }
        fn sell(&mut self, id: u32) {
            let i = self.inv.iter().position(|x| *x == id).unwrap();
            self.inv.remove(i);
            self.gold += (self.d.item(id).unwrap().total as f64 * 0.7).round();
        }
        fn titles(&self) -> Vec<&str> {
            self.events.iter().map(|e| e.title.as_str()).collect()
        }
    }

    #[test]
    fn buy_confirm_after_a_moment() {
        let mut g = Game::new(&[1038, 1037], 2000.0);
        g.buy(3031, &[1038, 1037]);
        g.wait(1.0);
        assert!(g.events.is_empty(), "not before it stayed {CONFIRM_SECS} s");
        assert!(g.tr.waiting());
        g.wait(3.0);
        g.wait(2.0);
        assert_eq!(g.titles(), vec!["Completed Infinity Edge"]);
        let e = &g.events[0];
        assert!((e.game_time - 61.0).abs() < 1e-6, "at the purchase, not the confirmation: {}", e.game_time);
        assert_eq!(e.icon.as_ref().map(|i| (i.kind.as_str(), i.id.as_str(), i.version.as_str())), Some(("item", "3031", "16.20.1")));
        assert!(e.facts.contains(&("Cost".into(), "3500 gold".into())));
        assert!(e.facts.contains(&("Item".into(), "1st finished item".into())));
        assert!(e.facts.iter().any(|(k, v)| k == "Built from" && v.contains("B. F. Sword")), "{:?}", e.facts);
    }

    #[test]
    fn buy_undo_buy_again_and_sell() {
        let mut g = Game::new(&[1038, 1037, 1018], 3000.0);
        // Buy, undo right away: no chip.
        g.buy(3031, &[1038, 1037, 1018]);
        g.wait(1.0);
        g.undo(3031, &[1038, 1037, 1018]);
        g.wait(1.0);
        g.wait(6.0);
        assert!(g.events.is_empty(), "undone before confirmation: {:?}", g.titles());
        // Buy again: one chip.
        g.buy(3031, &[1038, 1037, 1018]);
        g.wait(1.0);
        g.wait(6.0);
        assert_eq!(g.titles(), vec!["Completed Infinity Edge"]);
        // Undo after the confirmation (still in base): the chip is withdrawn.
        g.undo(3031, &[1038, 1037, 1018]);
        g.wait(1.0);
        assert!(g.events.is_empty(), "undone later: chip withdrawn");
        // Buy a third time, then sell it much later: the chip stays (it was bought).
        g.buy(3031, &[1038, 1037, 1018]);
        g.wait(1.0);
        g.wait(5.0);
        g.wait(300.0);
        g.sell(3031);
        g.wait(1.0);
        assert_eq!(g.titles(), vec!["Completed Infinity Edge"], "a sale keeps the chip");
        // Undo the sale: the item comes back, no second chip.
        let back = (g.d.item(3031).unwrap().total as f64 * 0.7).round();
        g.inv.push(3031);
        g.gold -= back;
        g.wait(1.0);
        g.wait(6.0);
        assert_eq!(g.titles(), vec!["Completed Infinity Edge"], "sale undone: no new chip");
        // A different item: second chip.
        g.gold += 3000.0;
        g.buy(3089, &[]);
        g.wait(1.0);
        g.wait(5.0);
        assert_eq!(g.titles(), vec!["Completed Infinity Edge", "Completed Rabadon's Deathcap"]);
        assert!(g.events[1].facts.contains(&("Item".into(), "2nd finished item".into())));
    }

    #[test]
    fn undo_seen_only_by_gold() {
        // Bought from gold only (no components): the undo shows as the full price coming back.
        let mut g = Game::new(&[], 4000.0);
        g.buy(3089, &[]);
        g.wait(1.0);
        g.wait(5.0);
        assert_eq!(g.events.len(), 1);
        g.undo(3089, &[]);
        g.wait(1.0);
        assert!(g.events.is_empty());
    }

    #[test]
    fn components_starters_and_the_start_inventory_make_no_chips() {
        // Already finished at the first read (app started mid-game): no chip.
        let mut g = Game::new(&[3031], 500.0);
        g.wait(6.0);
        g.gold += 2000.0;
        g.buy(1038, &[]);
        g.buy(1055, &[]);
        g.buy(2003, &[]);
        g.buy(1001, &[]);
        g.wait(1.0);
        g.wait(5.0);
        assert!(g.events.is_empty(), "{:?}", g.titles());
        // Tier-2 boots count.
        g.gold += 1100.0;
        g.buy(3006, &[1001]);
        g.wait(1.0);
        g.wait(5.0);
        assert_eq!(g.titles(), vec!["Completed Berserker's Greaves"]);
    }

    #[test]
    fn ordinals() {
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(2), "2nd");
        assert_eq!(ordinal(3), "3rd");
        assert_eq!(ordinal(4), "4th");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(22), "22nd");
    }
}
