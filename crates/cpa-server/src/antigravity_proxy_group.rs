//! Pure proxy-group selection and failed-node cooldown from Python
//! `src/storage/sqlite_manager.py::_select_proxy_group_node`.
//!
//! This is deliberately not a persistence layer. The caller must hold a
//! transaction while changing node cooldown, group cursor and account binding;
//! otherwise two requests could independently assign the same stale node.
//! No proxy URLs or account secrets are retained by this module.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    RoundRobin,
    Random,
    Failover,
}

impl Strategy {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "round_robin" => Some(Self::RoundRobin),
            "random" => Some(Self::Random),
            "failover" => Some(Self::Failover),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub id: i64,
    pub enabled: bool,
    pub strategy: Strategy,
    /// Python stores the cursor in its proxy_groups SQLite table.
    pub cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub id: i64,
    pub group_id: i64,
    pub enabled: bool,
    pub cooldown_until: Option<i64>,
    pub failure_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub node_id: i64,
    /// Persist this with the account binding in the same transaction.
    pub next_cursor: usize,
}

/// Choose from enabled, non-cooling nodes in increasing ID order, like Python.
/// `random_slot` is supplied by the caller's RNG, not a deterministic choice
/// in production. None means there is no safe node; NEVER silently use direct.
pub fn select(group: &Group, nodes: &[Node], now: i64, random_slot: usize) -> Option<Selection> {
    if !group.enabled {
        return None;
    }
    let mut available: Vec<_> = nodes
        .iter()
        .filter(|n| n.group_id == group.id && n.enabled)
        .filter(|n| n.cooldown_until.is_none_or(|until| until <= now))
        .collect();
    available.sort_unstable_by_key(|n| n.id);
    if available.is_empty() {
        return None;
    }
    let position = match group.strategy {
        Strategy::RoundRobin => group.cursor % available.len(),
        Strategy::Random => random_slot % available.len(),
        Strategy::Failover => 0,
    };
    let next_cursor = match group.strategy {
        Strategy::RoundRobin => (position + 1) % available.len(),
        _ => group.cursor,
    };
    Some(Selection {
        node_id: available[position].id,
        next_cursor,
    })
}

/// Plan one failed bound-node release and replacement. A future SQLite adapter
/// must persist the changed node, cursor, and cleared account binding atomically.
/// Failure is recorded even when every other node is unavailable.
pub fn cool_failed_and_select(
    group: &Group,
    nodes: &mut [Node],
    failed_node_id: i64,
    now: i64,
    cooldown_seconds: i64,
    random_slot: usize,
) -> Option<Selection> {
    // Invalid cooldowns must not immediately reintroduce a failed node.
    // Reject without mutating the caller's in-memory health snapshot.
    if cooldown_seconds <= 0 {
        return None;
    }
    let failed = nodes
        .iter_mut()
        .find(|n| n.group_id == group.id && n.id == failed_node_id)?;
    failed.failure_count = failed.failure_count.saturating_add(1);
    failed.cooldown_until = Some(now.saturating_add(cooldown_seconds));
    select(group, nodes, now, random_slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_cooldown_does_not_mutate_failed_node() {
        let mut candidates = nodes();
        let original = candidates.clone();
        for seconds in [0, -1, i64::MIN] {
            assert!(
                cool_failed_and_select(&group(Strategy::Failover, 0), &mut candidates, 10, 100, seconds, 0).is_none()
            );
            assert_eq!(candidates, original);
        }
    }

    #[test]
    fn stress_round_robin_selection_never_returns_ineligible_node() {
        let mut candidates = nodes();
        candidates[0].cooldown_until = Some(200);
        candidates[1].enabled = false;
        for cursor in 0..10_000 {
            let chosen = select(&group(Strategy::RoundRobin, cursor), &candidates, 100, cursor).unwrap();
            assert_eq!(chosen.node_id, 20);
            assert_eq!(chosen.next_cursor, 0);
        }
    }

    fn nodes() -> Vec<Node> {
        vec![
            Node {
                id: 30,
                group_id: 1,
                enabled: true,
                cooldown_until: None,
                failure_count: 0,
            },
            Node {
                id: 10,
                group_id: 1,
                enabled: true,
                cooldown_until: None,
                failure_count: 0,
            },
            Node {
                id: 20,
                group_id: 1,
                enabled: true,
                cooldown_until: None,
                failure_count: 0,
            },
            Node {
                id: 5,
                group_id: 2,
                enabled: true,
                cooldown_until: None,
                failure_count: 0,
            },
        ]
    }

    fn group(strategy: Strategy, cursor: usize) -> Group {
        Group {
            id: 1,
            enabled: true,
            strategy,
            cursor,
        }
    }

    #[test]
    fn round_robin_uses_sorted_eligible_nodes_and_advances_cursor() {
        let nodes = nodes();
        assert_eq!(
            select(&group(Strategy::RoundRobin, 0), &nodes, 100, 99),
            Some(Selection {
                node_id: 10,
                next_cursor: 1
            })
        );
        assert_eq!(
            select(&group(Strategy::RoundRobin, 1), &nodes, 100, 99),
            Some(Selection {
                node_id: 20,
                next_cursor: 2
            })
        );
        assert_eq!(
            select(&group(Strategy::RoundRobin, 2), &nodes, 100, 99),
            Some(Selection {
                node_id: 30,
                next_cursor: 0
            })
        );
    }

    #[test]
    fn disabled_and_cooling_nodes_are_not_selected() {
        let mut nodes = nodes();
        nodes[1].enabled = false;
        nodes[2].cooldown_until = Some(200);
        assert_eq!(
            select(&group(Strategy::RoundRobin, 0), &nodes, 199, 0).unwrap().node_id,
            30
        );
        assert_eq!(
            select(&group(Strategy::RoundRobin, 0), &nodes, 200, 0).unwrap().node_id,
            20
        );
        assert_eq!(
            select(
                &Group {
                    enabled: false,
                    ..group(Strategy::Failover, 0)
                },
                &nodes,
                200,
                0
            ),
            None
        );
        assert_eq!(select(&group(Strategy::Failover, 0), &[], 200, 0), None);
    }

    #[test]
    fn random_and_failover_do_not_advance_cursor() {
        let nodes = nodes();
        assert_eq!(
            select(&group(Strategy::Random, 4), &nodes, 100, 4),
            Some(Selection {
                node_id: 20,
                next_cursor: 4
            })
        );
        assert_eq!(
            select(&group(Strategy::Failover, 4), &nodes, 100, 99),
            Some(Selection {
                node_id: 10,
                next_cursor: 4
            })
        );
        assert_eq!(Strategy::parse("round_robin"), Some(Strategy::RoundRobin));
        assert_eq!(Strategy::parse("random"), Some(Strategy::Random));
        assert_eq!(Strategy::parse("unknown"), None);
    }

    #[test]
    fn cooling_failed_node_rebinds_without_falling_back_to_direct() {
        let mut nodes = nodes();
        let chosen = cool_failed_and_select(&group(Strategy::Failover, 0), &mut nodes, 10, 100, 300, 0);
        assert_eq!(chosen.unwrap().node_id, 20);
        let old = nodes.iter().find(|n| n.id == 10).unwrap();
        assert_eq!(old.cooldown_until, Some(400));
        assert_eq!(old.failure_count, 1);
        assert_eq!(
            select(&group(Strategy::Failover, 0), &nodes, 399, 0).unwrap().node_id,
            20
        );
        assert_eq!(
            select(&group(Strategy::Failover, 0), &nodes, 400, 0).unwrap().node_id,
            10
        );
    }

    #[test]
    fn no_available_replacement_preserves_cooldown_and_never_selects_other_group() {
        let mut nodes = nodes();
        nodes[0].enabled = false;
        nodes[2].enabled = false;
        assert_eq!(
            cool_failed_and_select(&group(Strategy::Failover, 0), &mut nodes, 10, 100, 300, 0),
            None
        );
        assert_eq!(nodes.iter().find(|n| n.id == 10).unwrap().cooldown_until, Some(400));
        assert_eq!(nodes.iter().find(|n| n.id == 5).unwrap().failure_count, 0);
        assert_eq!(
            cool_failed_and_select(&group(Strategy::Failover, 0), &mut nodes, 5, 100, 300, 0),
            None
        );
    }

    #[test]
    fn disabled_group_still_records_failed_binding_without_selecting() {
        let mut nodes = nodes();
        let group = Group {
            enabled: false,
            ..group(Strategy::RoundRobin, usize::MAX)
        };
        assert_eq!(cool_failed_and_select(&group, &mut nodes, 10, 100, 300, 0), None);
        assert_eq!(nodes.iter().find(|node| node.id == 10).unwrap().failure_count, 1);
        assert_eq!(
            nodes.iter().find(|node| node.id == 10).unwrap().cooldown_until,
            Some(400)
        );
        assert_eq!(
            select(&Group { enabled: true, ..group }, &nodes, 400, 0)
                .unwrap()
                .next_cursor,
            1
        );
    }
}
