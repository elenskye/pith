//! 计算工具：数由代码算，不让模型心算（`decisions/0030`）。精确十进制（事实调查的 `Dec`），不用浮点：0.1 + 0.2 = 0.3。
//! 认：加减乘除、括号、负号、百分数（12% = 0.12）、千分位逗号；`^` 只认非负整数次方。除法保留 8 位小数。

use research_engine::dec::Dec;

pub fn eval(expr: &str) -> Result<String, String> {
    let cleaned: String = expr.chars().map(|c| match c {
        '×' | '＊' => '*',
        '÷' | '／' => '/',
        '（' => '(',
        '）' => ')',
        '－' | '−' => '-',
        '＋' => '+',
        '％' => '%',
        c => c,
    }).collect();
    let tokens = lex(&cleaned)?;
    if tokens.is_empty() {
        return Err("没有要算的式子。".into());
    }
    let mut p = Parser { t: tokens, i: 0 };
    let v = p.expr()?;
    if p.i < p.t.len() {
        return Err(format!("式子在第 {} 个记号后面看不懂。", p.i));
    }
    Ok(v.trimmed().to_string())
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(Dec),
    Op(char),
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let mut num = String::new();
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || (chars[i] == ',' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())) {
                if chars[i] != ',' {
                    num.push(chars[i]);
                }
                i += 1;
            }
            let mut d = Dec::parse(&num).ok_or_else(|| format!("「{num}」不是数。"))?;
            if i < chars.len() && chars[i] == '%' {
                d = d.div(Dec::from_int(100)).ok_or("百分数算不了。")?.trimmed();
                i += 1;
            }
            out.push(Tok::Num(d));
        } else if "+-*/()^".contains(c) {
            out.push(Tok::Op(c));
            i += 1;
        } else {
            return Err(format!("式子里有认不得的字符「{c}」：只认数字、+ - * / ^、括号和 %。"));
        }
    }
    Ok(out)
}

struct Parser {
    t: Vec<Tok>,
    i: usize,
}

const OVERFLOW: &str = "数太大，算不了。";

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn op(&mut self, ops: &str) -> Option<char> {
        match self.peek() {
            Some(Tok::Op(c)) if ops.contains(*c) => {
                let c = *c;
                self.i += 1;
                Some(c)
            }
            _ => None,
        }
    }

    fn expr(&mut self) -> Result<Dec, String> {
        let mut v = self.term()?;
        while let Some(o) = self.op("+-") {
            let r = self.term()?;
            v = if o == '+' { v.add(r) } else { v.sub(r) }.ok_or(OVERFLOW)?;
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<Dec, String> {
        let mut v = self.power()?;
        while let Some(o) = self.op("*/") {
            let r = self.power()?;
            v = if o == '*' { v.mul(r).ok_or(OVERFLOW)? } else { v.div(r).ok_or("除数是 0，算不了。")? };
        }
        Ok(v)
    }

    fn power(&mut self) -> Result<Dec, String> {
        let base = self.unary()?;
        if self.op("^").is_some() {
            let e = self.unary()?;
            let n: u32 = e.trimmed().to_string().parse().map_err(|_| "次方只认 0 到 64 的整数。".to_string())?;
            if n > 64 {
                return Err("次方只认 0 到 64 的整数。".into());
            }
            let mut v = Dec::from_int(1);
            for _ in 0..n {
                v = v.mul(base).ok_or(OVERFLOW)?;
            }
            return Ok(v);
        }
        Ok(base)
    }

    fn unary(&mut self) -> Result<Dec, String> {
        if self.op("-").is_some() {
            return Dec::ZERO.sub(self.unary()?).ok_or_else(|| OVERFLOW.to_string());
        }
        if self.op("+").is_some() {
            return self.unary();
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Dec, String> {
        match self.peek().cloned() {
            Some(Tok::Num(d)) => {
                self.i += 1;
                Ok(d)
            }
            Some(Tok::Op('(')) => {
                self.i += 1;
                let v = self.expr()?;
                if self.op(")").is_none() {
                    return Err("括号没有配对。".into());
                }
                Ok(v)
            }
            _ => Err("式子不完整。".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::eval;

    #[test]
    fn exact_decimal_arithmetic() {
        assert_eq!(eval("0.1 + 0.2").unwrap(), "0.3");
        assert_eq!(eval("(1,234 - 1,000) / 1,000").unwrap(), "0.234");
        assert_eq!(eval("12% * 250").unwrap(), "30");
        assert_eq!(eval("-(3 - 5) * 2").unwrap(), "4");
        assert_eq!(eval("2^10").unwrap(), "1024");
        assert_eq!(eval("1 / 3").unwrap(), "0.33333333");
        assert_eq!(eval("（100－80）÷80×100").unwrap(), "25");
        assert!(eval("1 / 0").unwrap_err().contains("0"));
        assert!(eval("2 + abc").unwrap_err().contains("认不得"));
        assert!(eval("(1 + 2").unwrap_err().contains("括号"));
        assert!(eval("").is_err());
    }
}
