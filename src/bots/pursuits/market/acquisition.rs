use super::{DEFENSE, Goods, LIFE, SPEED, bounded_quote};
use crate::content::Content;
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

const DEXTERITY: i32 = 2636;
// Ordinary liquid savings exclude the historically priced resurrection amulet.
const CURRENCIES: [i32; 4] = [DEFENSE, SPEED, LIFE, DEXTERITY];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AcquisitionState {
    Funded { buy: [Goods; 2] },
    Saving { sale: Option<[Goods; 2]> },
    NoWholeTerms,
    Ineligible(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Acquisition {
    pub state: AcquisitionState,
    reserved: BTreeMap<i32, u8>,
}

impl Acquisition {
    pub(crate) fn reserved(&self, kind: i32) -> u8 {
        self.reserved.get(&kind).copied().unwrap_or(0)
    }

    pub(crate) fn spendable(&self, kind: i32, actual_held: usize) -> usize {
        actual_held.saturating_sub(usize::from(self.reserved(kind)))
    }
}

// Live aggregated spares exclude needs, keeps and locked trades; revalidate before acting because cached Vault is not spending authority.
pub(crate) fn plan(c: &Content, target: i32, spares: &[Goods]) -> Result<Acquisition> {
    ensure!(
        (0..=65535).contains(&target),
        "invalid acquisition target {target}"
    );
    let mut stock = BTreeMap::new();
    for g in spares {
        ensure!(
            (0..=65535).contains(&g.kind) && g.quantity > 0,
            "invalid acquisition spare {g:?}"
        );
        ensure!(
            stock.insert(g.kind, g.quantity).is_none(),
            "duplicate acquisition spare kind {}",
            g.kind
        );
    }
    let mut result = Acquisition {
        state: AcquisitionState::NoWholeTerms,
        reserved: BTreeMap::new(),
    };
    if let Err(reason) = c.item_values.quote_range(target) {
        result.state = AcquisitionState::Ineligible(reason.to_owned());
        return Ok(result);
    }
    // Unknown or unsafe stock is ineligible, not wealth with an invented zero price.
    let eligible: Vec<Goods> = stock
        .into_iter()
        .filter(|(kind, _)| c.item_values.quote_range(*kind).is_ok())
        .map(|(kind, quantity)| Goods { kind, quantity })
        .collect();
    let want = Goods {
        kind: target,
        quantity: 1,
    };
    if let Some(buy) = eligible
        .iter()
        .filter_map(|give| bounded_quote(give, &want, c))
        .min_by_key(quote_order)
    {
        result.reserved.insert(buy[0].kind, buy[0].quantity);
        result.state = AcquisitionState::Funded { buy };
        return Ok(result);
    }
    for g in &eligible {
        if CURRENCIES.contains(&g.kind) {
            result.reserved.insert(g.kind, g.quantity);
        }
    }
    let mut whole_terms = false;
    let mut sales = Vec::new();
    for kind in CURRENCIES {
        if c.item_values.quote_range(kind).is_err() {
            continue;
        }
        let Some(cost) = bounded_quote(&Goods { kind, quantity: 8 }, &want, c) else {
            continue;
        };
        whole_terms = true;
        // Never churn saved currency or the ambition itself to manufacture apparent progress.
        for give in &eligible {
            if give.kind != target
                && !CURRENCIES.contains(&give.kind)
                && let Some(sale) = bounded_quote(give, &cost[0], c)
            {
                sales.push(sale);
            }
        }
    }
    if whole_terms {
        let sale = sales.into_iter().min_by_key(quote_order);
        if let Some(q) = &sale {
            result.reserved.insert(q[0].kind, q[0].quantity);
        }
        result.state = AcquisitionState::Saving { sale };
    }
    Ok(result)
}

// Fewest native supplied items wins; canonical kinds break ties independently of input order.
fn quote_order(q: &[Goods; 2]) -> (u8, i32, i32) {
    (q[0].quantity, q[0].kind, q[1].kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::pursuits::market::fair;
    use std::path::Path;

    fn goods(kind: i32, quantity: u8) -> Goods {
        Goods { kind, quantity }
    }

    #[test]
    fn affordable_exact_target_uses_shared_fair_native_terms() {
        let c = Content::load(Path::new("data")).unwrap();
        let p = plan(&c, 2794, &[goods(2592, 16)]).unwrap();
        let AcquisitionState::Funded { buy } = &p.state else {
            panic!("expected native funded buy: {p:?}");
        };
        assert_eq!(*buy, [goods(2592, 3), goods(2794, 1)]);
        assert!(fair(buy, &c));
        assert!(buy.iter().all(|g| (1..=8).contains(&g.quantity)));
        assert_eq!(p.reserved(2592), 3);
        assert_eq!(p.spendable(2592, 16), 13);
        assert_eq!(p.spendable(2592, 2), 0);
        let high = plan(&c, 1826, &[goods(2793, 2)]).unwrap();
        assert!(matches!(high.state, AcquisitionState::Funded { .. }));
    }

    #[test]
    fn insufficient_unpriced_or_unsafe_stock_never_fabricates_funds() {
        let c = Content::load(Path::new("data")).unwrap();
        for stock in [
            vec![],
            vec![goods(2592, 2)],
            vec![goods(2592, 2), goods(2593, 4)],
            vec![goods(65535, 255), goods(2878, 8), goods(2806, 8)],
        ] {
            let p = plan(&c, 2794, &stock).unwrap();
            assert_eq!(p.state, AcquisitionState::Saving { sale: None });
            for g in stock {
                assert!(p.reserved(g.kind) <= g.quantity);
            }
        }
        for (target, reason) in [
            (65535, "unknown_catalog_item"),
            (2878, "runtime_trade_safety_restriction"),
            (2727, "strict_or_unbounded_price"),
            (2806, "additional_constraint_not_supported"),
        ] {
            let p = plan(&c, target, &[goods(2592, 16)]).unwrap();
            assert_eq!(p.state, AcquisitionState::Ineligible(reason.into()));
            assert_eq!(p.spendable(2592, 16), 16);
        }
        let p = plan(&c, 1826, &[goods(2592, 16)]).unwrap();
        assert_eq!(p.state, AcquisitionState::Saving { sale: None });
    }

    #[test]
    fn saving_reserves_only_real_cash_and_selected_sale_leaving_other_surplus_free() {
        let c = Content::load(Path::new("data")).unwrap();
        let stock = [goods(2592, 2), goods(2719, 4), goods(2585, 1)];
        let p = plan(&c, 2794, &stock).unwrap();
        let AcquisitionState::Saving { sale: Some(sale) } = &p.state else {
            panic!("expected sale toward concrete target: {p:?}");
        };
        assert_eq!(*sale, [goods(2719, 3), goods(2592, 1)]);
        assert!(fair(sale, &c));
        assert_eq!(p.reserved(2592), 2);
        assert_eq!(p.reserved(2719), 3);
        assert_eq!(p.spendable(2719, 4), 1);
        assert_eq!(p.spendable(2585, 1), 1);
        for g in &stock {
            assert!(p.reserved(g.kind) <= g.quantity);
        }
        let mut reversed = stock.clone();
        reversed.reverse();
        assert_eq!(plan(&c, 2794, &reversed).unwrap(), p);
        let after_sale = plan(&c, 2794, &[goods(2592, 3), goods(2719, 1)]).unwrap();
        assert!(matches!(after_sale.state, AcquisitionState::Funded { .. }));
        assert_eq!(after_sale.spendable(2719, 1), 1);
    }

    #[test]
    fn malformed_stock_and_target_fail_explicitly() {
        let c = Content::load(Path::new("data")).unwrap();
        for (stock, message) in [
            (vec![goods(2592, 0)], "invalid acquisition spare"),
            (vec![goods(-1, 1)], "invalid acquisition spare"),
            (vec![goods(65536, 1)], "invalid acquisition spare"),
            (
                vec![goods(2592, 1), goods(2592, 2)],
                "duplicate acquisition spare kind 2592",
            ),
        ] {
            assert!(
                plan(&c, 2794, &stock)
                    .unwrap_err()
                    .to_string()
                    .contains(message)
            );
        }
        assert!(
            plan(&c, -1, &[])
                .unwrap_err()
                .to_string()
                .contains("invalid acquisition target")
        );
    }
}
