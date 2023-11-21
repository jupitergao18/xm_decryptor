use crate::id3::v1::GENRE_LIST;
use std::borrow::Cow;
use std::mem::swap;

#[derive(Copy, Clone)]
pub struct Parser<'a>(&'a str);

type ParseFunc<P, T> = dyn Fn(&mut P) -> Result<T, ()>;

impl<'a> Parser<'a> {
    pub fn parse_tcon(s: &'a str) -> Cow<str> {
        let mut parser = Parser(s);
        let v1_genre_ids = match parser.one_or_more(&Self::content_type) {
            Ok(v) => v,
            Err(_) => return Cow::Borrowed(parser.0),
        };
        let trailer = parser.trailer();

        let strs: Vec<String> = v1_genre_ids.into_iter().chain(trailer).collect();
        Cow::Owned(strs.join(" "))
    }

    fn content_type(&mut self) -> Result<String, ()> {
        self.first_of([&Self::escaped_content_type, &Self::v1_content_type])
    }

    fn v1_content_type(&mut self) -> Result<String, ()> {
        self.expect("(")?;
        let t = self.first_of([
            &|p: &mut Self| p.expect("RX").map(|_| "Remix".to_string()),
            &|p: &mut Self| p.expect("CR").map(|_| "Cover".to_string()),
            &|p: &mut Self| {
                p.parse_number()
                    .map(|index| match GENRE_LIST.get(index as usize) {
                        Some(v1_genre) => v1_genre.to_string(),
                        None => format!("({})", index),
                    })
            },
        ])?;
        self.expect(")")?;
        Ok(t)
    }

    fn escaped_content_type(&mut self) -> Result<String, ()> {
        self.expect("((")?;
        let t = format!("({}", self.0);
        self.0 = "";
        Ok(t)
    }

    fn trailer(&mut self) -> Result<String, ()> {
        let mut tmp = "";
        swap(&mut tmp, &mut self.0);
        if tmp.is_empty() {
            return Err(());
        }
        Ok(tmp.to_string())
    }

    fn expect<'s>(&mut self, prefix: &'s str) -> Result<&'s str, ()> {
        if self.0.starts_with(prefix) {
            self.0 = &self.0[prefix.len()..];
            Ok(prefix)
        } else {
            Err(())
        }
    }

    fn one_or_more<T>(&mut self, func: &ParseFunc<Self, T>) -> Result<Vec<T>, ()> {
        let mut values = Vec::new();
        while let Ok(v) = func(self) {
            values.push(v);
        }
        if values.is_empty() {
            return Err(());
        }
        Ok(values)
    }

    fn first_of<T, const N: usize>(&mut self, funcs: [&ParseFunc<Self, T>; N]) -> Result<T, ()> {
        for func in funcs {
            let mut p = *self;
            if let Ok(v) = func(&mut p) {
                *self = p;
                return Ok(v);
            }
        }
        Err(())
    }

    fn parse_number(&mut self) -> Result<u32, ()> {
        let mut ok = false;
        let mut r = 0u32;
        while self.0.starts_with(|c: char| c.is_ascii_digit()) {
            ok = true;
            r = if let Some(r) = r
                .checked_mul(10)
                .and_then(|r| r.checked_add(u32::from(self.0.as_bytes()[0] - b'0')))
            {
                r
            } else {
                return Err(());
            };
            self.0 = &self.0[1..];
        }
        if ok {
            Ok(r)
        } else {
            Err(())
        }
    }
}
