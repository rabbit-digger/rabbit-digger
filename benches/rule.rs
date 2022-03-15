use criterion::{black_box, criterion_group, criterion_main, Criterion};
use futures::FutureExt;
use rabbit_digger::config::{self, Config};
use rd_interface::{config::resolve_net, registry::NetGetter, Address, Context, IntoDyn};
use rd_std::{
    rule::{matcher::MatchContext, rule_net::Rule},
    tests::TestNet,
};

fn build_rule(s: &config::Net, getter: NetGetter) -> Rule {
    let mut config = serde_json::from_value(s.opt.clone()).unwrap();
    resolve_net(&mut config, getter).unwrap();
    Rule::new(config).unwrap()
}

fn test_domain(rule: &Rule, ctx: &MatchContext, wanted: &str) {
    assert_eq!(
        rule.match_rule(ctx)
            .now_or_never()
            .unwrap()
            .unwrap()
            .1
            .target_name,
        wanted
    );
}

fn criterion_benchmark(c: &mut Criterion) {
    let content = std::fs::read_to_string("./blob/test/rule.yaml").unwrap();
    let config: Config = serde_yaml::from_str(&content).unwrap();
    let net_item = config.net.get("rule").unwrap();
    let test_net = TestNet::new().into_dyn();
    let net_getter: NetGetter = &|_| Some(test_net.clone());

    let rule = build_rule(net_item, net_getter);

    c.bench_function("build_net", |b| {
        b.iter(|| build_rule(black_box(net_item), net_getter))
    });

    // The middle one
    let addr = Address::Domain("google.com".to_string(), 12345);
    let ctx = MatchContext::from_context_address(&Context::new(), &addr).unwrap();
    c.bench_function("test_domain_google", |b| {
        b.iter(|| test_domain(&rule, &ctx, "\"🔰国外流量\""))
    });

    // The last one
    let addr = Address::Domain("www.zzzzzz.me".to_string(), 12345);
    let ctx = MatchContext::from_context_address(&Context::new(), &addr).unwrap();
    c.bench_function("test_domain_zzzzzz", |b| {
        b.iter(|| test_domain(&rule, &ctx, "\"local\""))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
