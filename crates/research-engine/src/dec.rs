//! 精确的十进制小数。核对和计算都用它，不用浮点：0.1 + 0.2 必须等于 0.3，「12.30%」必须等于「12.3%」。
//!
//! 值 = `m / 10^s`。统计数字的位数很少，i128 足够；溢出时返回 `None`，调用方当成「算不了」。

use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Copy, Debug)]
pub struct Dec {
    m: i128,
    s: u32,
}

/// 除法保留到小数点后几位（再往后四舍五入）
pub const DIV_PLACES: u32 = 8;
const MAX_SCALE: u32 = 18;

fn pow10(n: u32) -> Option<i128> {
    10i128.checked_pow(n)
}

// 加减乘除都可能溢出或除以 0，所以返回 Option，不实现 std::ops 的运算符
#[allow(clippy::should_implement_trait)]
impl Dec {
    pub const ZERO: Dec = Dec { m: 0, s: 0 };

    pub fn from_int(n: i64) -> Dec {
        Dec { m: n as i128, s: 0 }
    }

    /// 只认「可选负号 + 数字 + 可选小数」；千分位、单位由 `numbers` 先去掉。
    pub fn parse(text: &str) -> Option<Dec> {
        let t = text.trim();
        let (neg, body) = match t.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, t.strip_prefix('+').unwrap_or(t)),
        };
        let (int, frac) = body.split_once('.').unwrap_or((body, ""));
        if int.is_empty() && frac.is_empty() {
            return None;
        }
        if !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if int.len() + frac.len() > 30 || frac.len() as u32 > MAX_SCALE {
            return None;
        }
        let digits = format!("{int}{frac}");
        let m: i128 = if digits.is_empty() { 0 } else { digits.parse().ok()? };
        Some(Dec { m: if neg { -m } else { m }, s: frac.len() as u32 })
    }

    /// 写出来有几位小数（`12.30` 是 2 位）
    pub fn places(&self) -> u32 {
        self.s
    }

    pub fn is_zero(&self) -> bool {
        self.m == 0
    }

    pub fn is_negative(&self) -> bool {
        self.m < 0
    }

    /// 去掉末尾的 0：`12.30` → `12.3`
    pub fn trimmed(mut self) -> Dec {
        while self.s > 0 && self.m % 10 == 0 {
            self.m /= 10;
            self.s -= 1;
        }
        self
    }

    fn aligned(a: Dec, b: Dec) -> Option<(i128, i128, u32)> {
        let s = a.s.max(b.s);
        Some((a.m.checked_mul(pow10(s - a.s)?)?, b.m.checked_mul(pow10(s - b.s)?)?, s))
    }

    pub fn add(self, o: Dec) -> Option<Dec> {
        let (a, b, s) = Dec::aligned(self, o)?;
        Some(Dec { m: a.checked_add(b)?, s })
    }

    pub fn sub(self, o: Dec) -> Option<Dec> {
        let (a, b, s) = Dec::aligned(self, o)?;
        Some(Dec { m: a.checked_sub(b)?, s })
    }

    pub fn mul(self, o: Dec) -> Option<Dec> {
        let d = Dec { m: self.m.checked_mul(o.m)?, s: self.s + o.s };
        if d.s > MAX_SCALE { Some(d.round(MAX_SCALE)) } else { Some(d) }
    }

    /// 乘以 10 的 n 次方（万 = 4，亿 = 8）
    pub fn shift(self, n: u32) -> Option<Dec> {
        if n <= self.s {
            Some(Dec { m: self.m, s: self.s - n })
        } else {
            Some(Dec { m: self.m.checked_mul(pow10(n - self.s)?)?, s: 0 })
        }
    }

    /// 除法，结果保留 `DIV_PLACES` 位（四舍五入）。除数为 0 返回 `None`。
    pub fn div(self, o: Dec) -> Option<Dec> {
        if o.m == 0 {
            return None;
        }
        // (a / 10^sa) / (b / 10^sb) = a * 10^(sb + P) / (b * 10^sa) / 10^P
        let num = self.m.checked_mul(pow10(o.s + DIV_PLACES)?)?;
        let den = o.m.checked_mul(pow10(self.s)?)?;
        Some(Dec { m: div_round(num, den), s: DIV_PLACES })
    }

    /// 四舍五入到小数点后 `places` 位（.5 远离 0：2.345 → 2.35，-2.345 → -2.35）
    pub fn round(self, places: u32) -> Dec {
        if places >= self.s {
            return self;
        }
        match pow10(self.s - places) {
            Some(p) => Dec { m: div_round(self.m, p), s: places },
            None => self,
        }
    }

    /// 补 0 到正好 `places` 位小数（显示用）；位数更多时先四舍五入
    pub fn fixed(self, places: u32) -> String {
        let r = self.round(places);
        let padded = match pow10(places - r.s.min(places)) {
            Some(p) => Dec { m: r.m.saturating_mul(p), s: places },
            None => r,
        };
        padded.to_string()
    }
}

/// 整数除法，四舍五入（.5 远离 0）
fn div_round(num: i128, den: i128) -> i128 {
    let neg = (num < 0) != (den < 0);
    let (n, d) = (num.unsigned_abs(), den.unsigned_abs());
    let q = n / d;
    let r = n % d;
    let q = if r * 2 >= d { q + 1 } else { q } as i128;
    if neg { -q } else { q }
}

impl PartialEq for Dec {
    fn eq(&self, o: &Dec) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}

impl Eq for Dec {}

impl PartialOrd for Dec {
    fn partial_cmp(&self, o: &Dec) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Dec {
    fn cmp(&self, o: &Dec) -> Ordering {
        match Dec::aligned(*self, *o) {
            Some((a, b, _)) => a.cmp(&b),
            // 对齐时溢出：两个数差了几十个数量级，按符号和整数部分比
            None => {
                let (a, b) = (self.round(0), o.round(0));
                a.m.cmp(&b.m)
            }
        }
    }
}

impl fmt::Display for Dec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let neg = self.m < 0;
        let digits = self.m.unsigned_abs().to_string();
        let s = self.s as usize;
        let (int, frac) = if digits.len() > s { (digits[..digits.len() - s].to_string(), digits[digits.len() - s..].to_string()) } else { ("0".to_string(), format!("{digits:0>s$}")) };
        write!(f, "{}{int}", if neg { "-" } else { "" })?;
        if s > 0 {
            write!(f, ".{frac}")?;
        }
        Ok(())
    }
}

/// 存取用：写成字符串，读回来一位不差
impl serde::Serialize for Dec {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Dec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Dec, D::Error> {
        let s = String::deserialize(d)?;
        Dec::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("不是十进制小数：{s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Dec {
        Dec::parse(s).unwrap()
    }

    #[test]
    fn parses_and_prints() {
        for s in ["0", "12", "12.30", "-0.5", "0.001", "123456789012345678.123456"] {
            assert_eq!(d(s).to_string(), s);
        }
        assert_eq!(d("+3.5").to_string(), "3.5");
        assert_eq!(d(".5").to_string(), "0.5");
        assert_eq!(d("5.").to_string(), "5");
        for bad in ["", "-", ".", "1,234", "1e5", "12.3.4", "一", "1 2"] {
            assert!(Dec::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn equality_ignores_trailing_zeros() {
        assert_eq!(d("12.30"), d("12.3"));
        assert_ne!(d("12.31"), d("12.3"));
        assert!(d("-1") < d("0.5") && d("10") > d("9.99"));
        assert_eq!(d("12.300").trimmed().to_string(), "12.3");
        assert_eq!(d("100").trimmed().to_string(), "100");
    }

    #[test]
    fn exact_arithmetic() {
        assert_eq!(d("0.1").add(d("0.2")).unwrap(), d("0.3"));
        assert_eq!(d("12.3").sub(d("10.15")).unwrap().to_string(), "2.15");
        assert_eq!(d("1.5").mul(d("-2")).unwrap(), d("-3"));
        assert_eq!(d("1.2").shift(4).unwrap().to_string(), "12000");
        assert_eq!(d("1.23456").shift(2).unwrap().to_string(), "123.456");
        assert_eq!(d("1").div(d("3")).unwrap().to_string(), "0.33333333");
        assert_eq!(d("2").div(d("3")).unwrap().to_string(), "0.66666667");
        assert!(d("1").div(Dec::ZERO).is_none());
        assert_eq!(d("8520").div(d("7100")).unwrap().round(2).to_string(), "1.20");
    }

    #[test]
    fn rounds_half_away_from_zero() {
        assert_eq!(d("2.345").round(2).to_string(), "2.35");
        assert_eq!(d("-2.345").round(2).to_string(), "-2.35");
        assert_eq!(d("2.344").round(2).to_string(), "2.34");
        assert_eq!(d("0.5").round(0).to_string(), "1");
        assert_eq!(d("2.3").round(4).to_string(), "2.3");
        assert_eq!(d("2.3").fixed(2), "2.30");
        assert_eq!(d("2.345").fixed(1), "2.3");
        assert_eq!(d("7").fixed(0), "7");
    }

    #[test]
    fn serde_round_trip() {
        let j = serde_json::to_string(&d("12.30")).unwrap();
        assert_eq!(j, "\"12.30\"");
        let back: Dec = serde_json::from_str(&j).unwrap();
        assert_eq!(back.to_string(), "12.30");
        assert!(serde_json::from_str::<Dec>("\"abc\"").is_err());
    }
}
