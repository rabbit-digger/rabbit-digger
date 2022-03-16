use super::matcher::{MatchContext, Matcher, MaybeAsync};
use super::{config::AnyMatcher, matcher::MatcherBuilder};

impl Matcher for AnyMatcher {
    fn match_rule(&self, _match_context: &MatchContext) -> MaybeAsync<bool> {
        true.into()
    }
}

impl MatcherBuilder for AnyMatcher {
    fn build(self) -> Box<dyn Matcher> {
        Box::new(self)
    }
}

#[cfg(test)]
mod tests {
    use rd_interface::{Context, IntoAddress};

    use super::*;

    #[tokio::test]
    async fn test_any_matcher() {
        let matcher = AnyMatcher {};
        let mut match_context = MatchContext::from_context_address(
            &Context::new(),
            &"127.0.0.1:26666".into_address().unwrap(),
        )
        .unwrap();
        assert!(matcher.match_rule(&mut match_context).await);
    }
}
