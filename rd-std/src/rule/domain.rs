use std::{collections::BTreeMap, convert::TryFrom};

use super::matcher::{MatchContext, Matcher, MaybeAsync};
use super::{
    config::{DomainMatcher, DomainMatcherMethod as Method},
    matcher::MatcherBuilder,
};
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use anyhow::Result;
use stats_alloc::{Region, StatsAlloc};

pub struct DomainMatcherInner {
    method: Method,
    // domain: Vec<Box<[u8]>>,
    is_prefix: BTreeMap<usize, bool>,
    ac: AhoCorasick<u32>,
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
    fn build(self) -> Box<dyn Matcher> {
        Box::new(DomainMatcherInner::new(self))
        // Box::new(self)
    }
}

impl DomainMatcherInner {
    pub fn debug(cfg: DomainMatcher, g: &StatsAlloc<std::alloc::System>) -> Self {
        let mut reg = Region::new(g);

        let mut is_prefix = BTreeMap::new();
        let domain = cfg
            .domain
            .into_vec()
            .into_iter()
            .map(|i| i.into_bytes())
            .map(|mut v| {
                v.reverse();
                v.into_boxed_slice()
            })
            .collect::<Vec<_>>();

        println!("Domain #1 {:#?}", reg.change_and_reset());

        let without_plus = domain
            .iter()
            .enumerate()
            .inspect(|(id, i)| {
                if i.ends_with(b".+") {
                    is_prefix.insert(*id, true);
                }
            })
            .map(|(_, i)| i.strip_suffix(b".+").unwrap_or(&i));

        println!("Domain #2 {:#?}", reg.change_and_reset());

        let ac = AhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostLongest)
            .prefilter(false)
            .dense_depth(0)
            .build_with_size(without_plus)
            .unwrap();

        println!("Domain #3 {:#?}", reg.change_and_reset());

        DomainMatcherInner {
            method: cfg.method,
            // domain,
            is_prefix,
            ac,
        }
    }
    pub fn new(cfg: DomainMatcher) -> Self {
        let mut is_prefix = BTreeMap::new();
        let domain = cfg
            .domain
            .into_vec()
            .into_iter()
            .map(|i| i.into_bytes())
            .map(|mut v| {
                v.reverse();
                v.into_boxed_slice()
            })
            .collect::<Vec<_>>();

        let without_plus = domain
            .iter()
            .enumerate()
            .inspect(|(id, i)| {
                if i.ends_with(b".+") {
                    is_prefix.insert(*id, true);
                }
            })
            .map(|(_, i)| i.strip_suffix(b".+").unwrap_or(&i));

        let ac = AhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostLongest)
            .prefilter(false)
            .build_with_size(without_plus)
            .unwrap();

        DomainMatcherInner {
            method: cfg.method,
            // domain,
            is_prefix,
            ac,
        }
    }
    fn test(&self, domain: &str) -> bool {
        let mut domain = domain.as_bytes().to_vec();
        domain.reverse();
        let len = domain.len();
        for i in self.ac.find_iter(&domain) {
            // let p = &self.domain[i.pattern()];
            let is_prefix = *self.is_prefix.get(&i.pattern()).unwrap_or(&false);
            match self.method {
                Method::Keyword => return true,
                Method::Suffix if i.start() == len => return true,
                Method::Match if (i.start() == 0 && i.end() == len) => return true,
                Method::Match
                    if (i.end() < len
                        && (is_prefix && domain[i.end()] == b'.')
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
        let inner = DomainMatcherInner::new(DomainMatcher {
            method: Method::Match,
            domain: vec!["+.google.com".to_string()].into(),
        });
        assert!(inner.test("google.com"));

        let inner = DomainMatcherInner::new(DomainMatcher {
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
