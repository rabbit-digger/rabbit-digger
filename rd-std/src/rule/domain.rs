use std::convert::TryFrom;

use super::matcher::{MatchContext, Matcher, MaybeAsync};
use super::{
    config::{DomainMatcher, DomainMatcherMethod as Method},
    matcher::MatcherBuilder,
};
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use anyhow::Result;

struct DomainMatcherInner {
    method: Method,
    domain: Vec<String>,
    ac: AhoCorasick,
}

impl TryFrom<String> for Method {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(match value.as_ref() {
            "keyword" => Method::Keyword,
            "suffix" => Method::Suffix,
            "match" => Method::Match,
            _ => return Err(anyhow::anyhow!("Unsupported method: {}", value)),
        })
    }
}

impl MatcherBuilder for DomainMatcher {
    fn build(&self) -> Box<dyn Matcher> {
        Box::new(DomainMatcherInner::new(self))
    }
}

struct ReverseIter<'b, I>(I, &'b Vec<u8>);

impl<'b, I> Iterator for ReverseIter<'b, I>
where
    I: Iterator<Item = &'b [u8]>,
{
    type Item = &'b [u8];

    fn next(&mut self) -> Option<Self::Item> {
        Some(&self.1[..])
    }
}

impl DomainMatcherInner {
    fn new(cfg: &DomainMatcher) -> Self {
        let domain = cfg.domain.clone().into_vec();
        let without_plus = domain
            .iter()
            .map(|i| i.strip_prefix("+.").unwrap_or(&i))
            .map(|i| i.as_bytes())
            .map(|b| {
                let mut b = b.to_vec();
                b.reverse();
                b
            });

        let ac = AhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostLongest)
            .build(without_plus);

        DomainMatcherInner {
            method: cfg.method.clone(),
            domain,
            ac,
        }
    }
    fn test(&self, domain: &str) -> bool {
        let mut domain = domain.as_bytes().to_vec();
        domain.reverse();
        let len = domain.len();
        for i in self.ac.find_iter(&domain) {
            let p = &self.domain[i.pattern()];
            match self.method {
                Method::Keyword => return true,
                Method::Suffix if i.start() == len => return true,
                Method::Match if (i.start() == 0 && i.end() == len) => return true,
                Method::Match
                    if (i.end() < len
                        && (p.starts_with("+.") && domain[i.end()] == b'.')
                        && i.start() == 0) =>
                {
                    return true
                }
                _ => return false,
            }
        }
        false
    }
}

impl Matcher for DomainMatcherInner {
    fn match_rule(&self, match_context: &MatchContext) -> MaybeAsync<bool> {
        match match_context.get_domain() {
            Some((domain, _)) => self.test(domain),
            // if it's not a domain, pass it.
            None => false,
        }
        .into()
    }
}

impl DomainMatcher {
    fn test(&self, domain: &str) -> bool {
        match self.method {
            Method::Keyword => self.domain.iter().any(|d| domain.contains(d)),
            Method::Match => self.domain.iter().any(|d| d == domain),
            Method::Suffix => self.domain.iter().any(|d| {
                if d.starts_with("+.") {
                    d.strip_prefix('+')
                        .map(|i| domain.ends_with(i))
                        .unwrap_or(false)
                        || d.strip_prefix("+.")
                            .map(|d| domain.ends_with(d))
                            .unwrap_or(false)
                } else {
                    domain.ends_with(d)
                }
            }),
        }
    }
}

impl Matcher for DomainMatcher {
    fn match_rule(&self, match_context: &MatchContext) -> MaybeAsync<bool> {
        match match_context.get_domain() {
            Some((domain, _)) => self.test(domain),
            // if it's not a domain, pass it.
            None => false,
        }
        .into()
    }
}

#[cfg(test)]
mod tests {
    use rd_interface::{Context, IntoAddress};

    use super::*;

    async fn match_addr(address: &str, matcher: &DomainMatcher) -> bool {
        let mut match_context =
            MatchContext::from_context_address(&Context::new(), &address.into_address().unwrap())
                .unwrap();
        matcher.match_rule(&mut match_context).await
    }

    #[test]
    fn test_new() {
        let inner = DomainMatcherInner::new(&DomainMatcher {
            method: Method::Match,
            domain: vec!["+.google.com".to_string()].into(),
        });
        assert!(inner.test("google.com"));

        let inner = DomainMatcherInner::new(&DomainMatcher {
            method: Method::Match,
            domain: vec!["+.zzzzzz.me".to_string()].into(),
        });
        assert!(inner.test("www.zzzzzz.me"));
    }

    #[tokio::test]
    async fn test_domain_matcher() {
        // test keyword
        let matcher = DomainMatcher {
            domain: vec!["example".to_string()].into(),
            method: Method::Keyword,
        };
        assert!(match_addr("example.com:26666", &matcher).await);
        assert!(!match_addr("exampl.com:26666", &matcher).await);

        // test match
        let matcher = DomainMatcher {
            domain: vec!["example.com".to_string()].into(),
            method: Method::Match,
        };
        assert!(match_addr("example.com:26666", &matcher).await);
        assert!(!match_addr("sub.example.com:26666", &matcher).await);

        // test suffix
        let matcher = DomainMatcher {
            domain: vec![".com".to_string()].into(),
            method: Method::Suffix,
        };
        assert!(match_addr("example.com:26666", &matcher).await);
        assert!(!match_addr("example.cn:26666", &matcher).await);

        // test suffix with +
        let matcher = DomainMatcher {
            domain: vec!["+.com".to_string()].into(),
            method: Method::Suffix,
        };
        assert!(match_addr("example.com:26666", &matcher).await);
        assert!(match_addr("sub.example.com:26666", &matcher).await);
        assert!(!match_addr("example.cn:26666", &matcher).await);
    }
}
