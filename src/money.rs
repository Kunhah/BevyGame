//! Edo-period currency.
//!
//! Money is stored internally as a single base-unit integer (`mon`, the copper
//! coin of everyday Edo life) and displayed as a tiered breakdown of
//! ryō / bu / shu / mon. It is a transparent newtype over `u32` so existing
//! arithmetic and comparisons against plain `u32` amounts keep compiling — the
//! whole codebase counts in `mon`, this type only changes how it reads on screen.
//!
//! Denomination ratios (placeholders — tune to taste):
//!   1 ryō (両) = 4 bu (分);  1 bu = 4 shu (朱);  1 shu = `MON_PER_SHU` mon (文).
//!
//! Note: the spiritual "favor" currency [`crate::quests::MerchantCoins`] is a
//! separate thing and is intentionally NOT a `Money`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};

/// Copper mon per shu. Historically a ryō was worth thousands of mon; this is a
/// round, easily-tuned placeholder (=> 1 bu = 1000 mon, 1 ryō = 4000 mon).
pub const MON_PER_SHU: u32 = 250;
pub const SHU_PER_BU: u32 = 4;
pub const BU_PER_RYO: u32 = 4;

pub const MON_PER_BU: u32 = MON_PER_SHU * SHU_PER_BU;
pub const MON_PER_RYO: u32 = MON_PER_BU * BU_PER_RYO;

/// An amount of money, in `mon`.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Money(pub u32);

impl Money {
    pub const ZERO: Money = Money(0);

    #[inline]
    pub const fn mon(&self) -> u32 {
        self.0
    }

    /// Split into (ryō, bu, shu, mon).
    pub fn breakdown(self) -> (u32, u32, u32, u32) {
        let mut rem = self.0;
        let ryo = rem / MON_PER_RYO;
        rem %= MON_PER_RYO;
        let bu = rem / MON_PER_BU;
        rem %= MON_PER_BU;
        let shu = rem / MON_PER_SHU;
        let mon = rem % MON_PER_SHU;
        (ryo, bu, shu, mon)
    }

    /// Tiered display, e.g. `"1両2分1朱120文"`. Omits zero tiers; shows `"0文"`
    /// when empty.
    pub fn format_tiered(self) -> String {
        let (ryo, bu, shu, mon) = self.breakdown();
        let mut out = String::new();
        if ryo > 0 {
            out.push_str(&format!("{ryo}両"));
        }
        if bu > 0 {
            out.push_str(&format!("{bu}分"));
        }
        if shu > 0 {
            out.push_str(&format!("{shu}朱"));
        }
        if mon > 0 || out.is_empty() {
            out.push_str(&format!("{mon}文"));
        }
        out
    }

    /// Compact form for the HUD (currently the same tiered string).
    pub fn format_short(self) -> String {
        self.format_tiered()
    }

    #[inline]
    pub fn saturating_add(self, rhs: u32) -> Money {
        Money(self.0.saturating_add(rhs))
    }

    #[inline]
    pub fn saturating_sub(self, rhs: u32) -> Money {
        Money(self.0.saturating_sub(rhs))
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format_tiered())
    }
}

impl fmt::Debug for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Money({} mon)", self.0)
    }
}

impl From<u32> for Money {
    #[inline]
    fn from(v: u32) -> Self {
        Money(v)
    }
}

// --- Interop with bare `u32` amounts (the codebase counts in mon) ---

impl PartialEq<u32> for Money {
    #[inline]
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<u32> for Money {
    #[inline]
    fn partial_cmp(&self, other: &u32) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl Add<u32> for Money {
    type Output = Money;
    #[inline]
    fn add(self, rhs: u32) -> Money {
        Money(self.0 + rhs)
    }
}

impl Sub<u32> for Money {
    type Output = Money;
    #[inline]
    fn sub(self, rhs: u32) -> Money {
        Money(self.0 - rhs)
    }
}

impl Mul<u32> for Money {
    type Output = Money;
    #[inline]
    fn mul(self, rhs: u32) -> Money {
        Money(self.0 * rhs)
    }
}

impl Div<u32> for Money {
    type Output = Money;
    #[inline]
    fn div(self, rhs: u32) -> Money {
        Money(self.0 / rhs)
    }
}

impl AddAssign<u32> for Money {
    #[inline]
    fn add_assign(&mut self, rhs: u32) {
        self.0 += rhs;
    }
}

impl SubAssign<u32> for Money {
    #[inline]
    fn sub_assign(&mut self, rhs: u32) {
        self.0 -= rhs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakdown_splits_tiers() {
        // 1 ryō + 2 bu + 1 shu + 120 mon = 4000 + 2000 + 250 + 120 = 6370 mon
        assert_eq!(Money(6370).breakdown(), (1, 2, 1, 120));
        assert_eq!(Money(0).breakdown(), (0, 0, 0, 0));
        assert_eq!(Money(MON_PER_RYO).breakdown(), (1, 0, 0, 0));
    }

    #[test]
    fn format_omits_zero_tiers_but_keeps_mon_when_empty() {
        assert_eq!(Money(0).format_tiered(), "0文");
        assert_eq!(Money(MON_PER_RYO).format_tiered(), "1両");
        assert_eq!(Money(6370).format_tiered(), "1両2分1朱120文");
    }

    #[test]
    fn u32_interop() {
        let mut m = Money(100);
        m += 50;
        assert_eq!(m, Money(150));
        assert!(m < 200u32);
        assert!(m >= 150u32);
        assert_eq!(m.saturating_sub(1000), Money(0));
    }
}
