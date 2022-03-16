use std::io::Read;
use std::net::IpAddr;

use super::matcher::{MatchContext, Matcher, MaybeAsync};
use super::{config::GeoIpMatcher, matcher::MatcherBuilder};
use flate2::read::GzDecoder;
use maxminddb::geoip2;
use once_cell::sync::OnceCell;
use tar::Archive;

// Update this when blob is updated
static GEOIP_TAR_GZ: &[u8] = include_bytes!("../../../blob/GeoLite2-Country_20210622.tar.gz");
static MMDB_PATH: &str = "GeoLite2-Country_20210622/GeoLite2-Country.mmdb";
static GEOIP_DB: OnceCell<maxminddb::Reader<Box<[u8]>>> = OnceCell::new();

pub fn get_reader() -> &'static maxminddb::Reader<Box<[u8]>> {
    // TODO: don't use expect
    GEOIP_DB.get_or_init(|| {
        let tar = GzDecoder::new(GEOIP_TAR_GZ);
        let mut archive = Archive::new(tar);
        let entries = archive.entries().expect("Failed to read tar");
        let mut mmdb = entries
            .filter_map(|i| i.ok())
            .find(|i| i.path().expect("Failed to read path").as_os_str() == MMDB_PATH)
            .expect("Failed to find mmdb in .tar.gz");
        let mut mmdb_buf = Vec::new();
        mmdb.read_to_end(&mut mmdb_buf)
            .expect("Failed to read mmdb");
        let mmdb_buf = mmdb_buf.into_boxed_slice();
        let reader =
            maxminddb::Reader::from_source(mmdb_buf).expect("Failed to read mmdb from source");
        reader
    })
}

impl GeoIpMatcher {
    fn test(&self, ip: impl Into<IpAddr>) -> bool {
        let ip = ip.into();
        let reader = get_reader();
        let result: Result<geoip2::Country, _> = reader.lookup(ip);
        match result {
            Ok(geoip2::Country {
                country:
                    Some(geoip2::model::Country {
                        iso_code: Some(country),
                        ..
                    }),
                ..
            }) => country == self.country,
            Err(e) => {
                tracing::debug!("Failed to lookup country for ip: {}, reason: {:?}", ip, e);
                false
            }
            _ => {
                // no message
                false
            }
        }
    }
}

impl MatcherBuilder for GeoIpMatcher {
    fn build(self) -> Box<dyn Matcher> {
        Box::new(self)
    }
}

impl Matcher for GeoIpMatcher {
    fn match_rule(&self, match_context: &MatchContext) -> MaybeAsync<bool> {
        match match_context.get_socket_addr() {
            Some(addr) => self.test(addr.ip()),
            None => false,
        }
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rd_interface::{Address, Context};

    #[tokio::test]
    async fn test_cn() {
        let matcher = GeoIpMatcher {
            country: "CN".to_string(),
        };
        assert!(
            matcher
                .match_rule(
                    &MatchContext::from_context_address(
                        &Context::new(),
                        &Address::SocketAddr("114.114.114.114:53".parse().unwrap())
                    )
                    .unwrap()
                )
                .await
        );
        assert!(
            !matcher
                .match_rule(
                    &MatchContext::from_context_address(
                        &Context::new(),
                        &Address::SocketAddr("1.1.1.1:53".parse().unwrap())
                    )
                    .unwrap()
                )
                .await
        );
    }
}
