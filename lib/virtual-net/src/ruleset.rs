use std::net::{IpAddr, SocketAddr};
use std::ops::RangeInclusive;
use std::str::FromStr;
use std::sync::Arc;

use ipnet::{Ipv4Net, Ipv6Net};
use iprange::IpRange;

#[derive(Debug, Clone)]
pub enum IPRange {
    IPV4Range(IpRange<Ipv4Net>),
    IPV6Range(IpRange<Ipv6Net>),
}

impl IPRange {
    pub fn matches(&self, ip: IpAddr) -> bool {
        match (self, ip) {
            (IPRange::IPV4Range(v4_range), IpAddr::V4(v4)) => v4_range.contains(&v4),
            (IPRange::IPV6Range(v6_range), IpAddr::V6(v6)) => v6_range.contains(&v6),
            _ => false,
        }
    }
}

impl FromStr for IPRange {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ip_range = if s.contains(':') {
            let ip = Ipv6Net::from_str(s)?;
            let mut ip_range = IpRange::<Ipv6Net>::new();
            ip_range.add(ip);

            IPRange::IPV6Range(ip_range)
        } else {
            let ip = Ipv4Net::from_str(s)?;
            let mut ip_range = IpRange::<Ipv4Net>::new();
            ip_range.add(ip);

            IPRange::IPV4Range(ip_range)
        };

        Ok(ip_range)
    }
}

#[derive(Debug, Clone)]
pub enum IPRule {
    All,
    IP(IpAddr),
    IPRange(IPRange),
}

impl IPRule {
    pub fn matches(&self, ip: IpAddr) -> bool {
        match (self, ip) {
            (IPRule::All, _) => true,
            (IPRule::IP(allowed_ip), IpAddr::V4(v4)) => *allowed_ip == v4,
            (IPRule::IP(allowed_ip), IpAddr::V6(v6)) => *allowed_ip == v6,
            (IPRule::IPRange(ip_range), _) => ip_range.matches(ip),
        }
    }
}

impl FromStr for IPRule {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ip_rule = if s == "*" {
            IPRule::All
        } else if s.contains('/') {
            IPRule::IPRange(IPRange::from_str(s)?)
        } else {
            IPRule::IP(IpAddr::from_str(s)?)
        };

        Ok(ip_rule)
    }
}

#[derive(Debug, Clone)]
pub enum PortRule {
    All,
    Port(u16),
    PortRange(RangeInclusive<u16>),
}

impl PortRule {
    pub fn matches(&self, port: u16) -> bool {
        match self {
            PortRule::All => true,
            PortRule::Port(allowed_port) => *allowed_port == port,
            PortRule::PortRange(port_range) => port_range.contains(&port),
        }
    }
}

impl FromStr for PortRule {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let port_rule = if s == "*" {
            PortRule::All
        } else if s.contains('-') {
            let (start, end) = s.split_once('-').unwrap();

            let (start, end) = (start.parse()?, end.parse()?);

            PortRule::PortRange(start..=end)
        } else {
            PortRule::Port(s.parse()?)
        };

        Ok(port_rule)
    }
}

#[derive(Debug, Clone)]
pub enum DomainRule {
    Domain(String),
    DomainGlob(String),
}

impl DomainRule {
    pub fn matches(&self, domain: impl AsRef<str>) -> bool {
        let domain = domain.as_ref();

        match self {
            DomainRule::Domain(allowed_domain) => allowed_domain == domain,
            DomainRule::DomainGlob(allowed_glob) => domain.ends_with(allowed_glob),
        }
    }
}

impl FromStr for DomainRule {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "*.*" {
            // this will match every domain
            return Ok(DomainRule::DomainGlob("".to_string()));
        }

        let domain_rule = if let Some(domain) = s.strip_prefix('*') {
            DomainRule::DomainGlob(domain.to_string())
        } else {
            DomainRule::Domain(s.to_string())
        };

        Ok(domain_rule)
    }
}

#[derive(Debug, Clone)]
pub enum Rule {
    IPAndPort {
        ip_rule: IPRule,
        port_rule: PortRule,
    },
    Domain {
        domain_rule: DomainRule,

        // this port is used during rule expansion
        port_rule: PortRule,
    },
    Negative(Arc<Rule>),
}

impl Rule {
    pub fn allows_ip(&self, ip: IpAddr) -> bool {
        match self {
            Rule::IPAndPort { ip_rule, .. } => ip_rule.matches(ip),
            _ => false,
        }
    }

    pub fn allows_port(&self, port: u16) -> bool {
        match self {
            Rule::IPAndPort { port_rule, .. } => port_rule.matches(port),
            _ => false,
        }
    }

    pub fn allows_domain(&self, domain: impl AsRef<str>) -> bool {
        match self {
            Rule::Domain { domain_rule, .. } => domain_rule.matches(domain),
            _ => false,
        }
    }

    pub fn blocks_ip(&self, ip: IpAddr) -> bool {
        match self {
            Rule::Negative(rule) => rule.allows_ip(ip),
            _ => false,
        }
    }

    pub fn blocks_port(&self, port: u16) -> bool {
        match self {
            Rule::Negative(rule) => rule.allows_port(port),
            _ => false,
        }
    }

    pub fn blocks_domain(&self, domain: impl AsRef<str>) -> bool {
        match self {
            Rule::Negative(rule) => rule.allows_domain(domain),
            _ => false,
        }
    }
}

impl FromStr for Rule {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // !rule
        // TODO(amin): handle negative rules
        let mut negative = false;
        let s = s
            .strip_prefix('!')
            .inspect(|_| {
                negative = true;
            })
            .unwrap_or(s);

        let rule = if s.starts_with('[') {
            // ipv6 address and port
            let (ip, port) = s.rsplit_once(':').unwrap();

            let start = ip.find('[').unwrap();
            let end = ip.find(']').unwrap();

            Rule::IPAndPort {
                ip_rule: IPRule::from_str(&s[start + 1..end])?,
                port_rule: PortRule::from_str(port)?,
            }
        } else if s.matches(':').count() > 1 {
            // ipv6
            Rule::IPAndPort {
                ip_rule: IPRule::from_str(s)?,
                port_rule: PortRule::All,
            }
        } else if s.contains(':') {
            // ipv4 or domain and port
            let (ip_or_domain, port) = s.rsplit_once(':').unwrap();

            let port_rule = PortRule::from_str(port)?;
            if let Ok(ip_rule) = IPRule::from_str(ip_or_domain) {
                Rule::IPAndPort { ip_rule, port_rule }
            } else {
                Rule::Domain {
                    domain_rule: DomainRule::from_str(ip_or_domain)?,
                    port_rule,
                }
            }
        } else {
            // either an ipv4 or a domain
            if let Ok(ip_rule) = IPRule::from_str(s) {
                Rule::IPAndPort {
                    ip_rule,
                    port_rule: PortRule::All,
                }
            } else if let Ok(domain_rule) = DomainRule::from_str(s) {
                Rule::Domain {
                    domain_rule,
                    port_rule: PortRule::All,
                }
            } else {
                anyhow::bail!("failed to parse rule: {}", s);
            }
        };

        let rule = if negative {
            Rule::Negative(Arc::new(rule))
        } else {
            rule
        };

        Ok(rule)
    }
}

#[derive(Debug, Clone)]
pub struct RuleSet {
    rules: Vec<Rule>,
}

impl RuleSet {
    pub fn matches_ip(&self, ip: IpAddr) -> bool {
        self.rules.iter().any(|rule| rule.allows_ip(ip))
    }

    pub fn matches_port(&self, port: u16) -> bool {
        self.rules.iter().any(|rule| rule.allows_port(port))
    }

    pub fn matches_socket_addr(&self, socket_addr: SocketAddr) -> bool {
        self.matches_ip(socket_addr.ip()) && self.matches_port(socket_addr.port())
    }

    pub fn matches_domain(&self, domain: impl AsRef<str>) -> bool {
        let domain = domain.as_ref();

        self.rules.iter().any(|rule| rule.allows_domain(domain))
    }
}

impl FromStr for RuleSet {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let rules = s
            .split(',')
            .map(|s| Rule::from_str(s.trim()))
            .collect::<Result<Vec<_>, anyhow::Error>>()?;

        Ok(Self { rules })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn all_ips() {
        let rule = IPRule::from_str("*").unwrap();

        assert!(rule.matches("192.168.1.0".parse().unwrap()));
        assert!(rule.matches("2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn single_ipv4() {
        let rule = IPRule::from_str("192.168.1.0").unwrap();

        let ip_addr: IpAddr = "192.168.1.0".parse().unwrap();
        assert!(rule.matches(ip_addr));

        let ip_addr: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(!rule.matches(ip_addr));
    }

    #[test]
    fn ipv4_range() {
        let rule = IPRule::from_str("192.168.1.0/24").unwrap();

        let matches = vec![
            "192.168.1.1",
            "192.168.1.0",
            "192.168.1.255",
            "192.168.1.100",
            "192.168.1.50",
        ];

        let non_matches = vec![
            "192.168.2.0",
            "192.167.1.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.0.255",
        ];

        for ip in matches {
            let ip_addr: IpAddr = ip.parse().unwrap();
            assert!(rule.matches(ip_addr));
        }

        for ip in non_matches {
            let ip_addr: IpAddr = ip.parse().unwrap();
            assert!(!rule.matches(ip_addr));
        }
    }

    #[test]
    fn single_ipv6() {
        let rule = IPRule::from_str("2001:db8::1").unwrap();

        assert!(rule.matches("2001:db8::1".parse().unwrap()));
        assert!(!rule.matches("2001:db7::1".parse().unwrap()));
    }

    #[test]
    fn ipv6_range() {
        let rule = IPRule::from_str("2001:db8::/32").unwrap();

        let matches = vec![
            "2001:db8::1",
            "2001:db8::",
            "2001:db8:0:0:0:0:0:1234",
            "2001:db8::abcd",
            "2001:db8::ffff",
        ];

        let non_matches = vec![
            "2001:db9::",
            "2001:db7::1",
            "2001:dead::1",
            "fe80::1",
            "::1",
        ];

        for ip in matches {
            let ip_addr: IpAddr = ip.parse().unwrap();
            assert!(rule.matches(ip_addr));
        }

        for ip in non_matches {
            let ip_addr: IpAddr = ip.parse().unwrap();
            assert!(!rule.matches(ip_addr));
        }
    }

    #[test]
    fn all_ports() {
        let rule = PortRule::from_str("*").unwrap();

        assert!(rule.matches(80));
    }

    #[test]
    fn single_port() {
        let rule = PortRule::from_str("80").unwrap();

        assert!(!rule.matches(79));
        assert!(rule.matches(80));
        assert!(!rule.matches(81));
    }

    #[test]
    fn port_range() {
        let rule = PortRule::from_str("80-100").unwrap();

        assert!(!rule.matches(79));
        for port in 80..=100 {
            assert!(rule.matches(port));
        }
        assert!(!rule.matches(101));
    }

    #[test]
    fn all_domain() {
        let rule = DomainRule::from_str("*.*").unwrap();

        assert!(rule.matches("a.b.c"));
        assert!(rule.matches("b.c"));
    }

    #[test]
    fn single_domain() {
        let rule = DomainRule::from_str("a.b.c").unwrap();

        assert!(rule.matches("a.b.c"));
        assert!(!rule.matches("b.c"));
    }

    #[test]
    fn domain_glob() {
        let rule = DomainRule::from_str("*.b.c").unwrap();

        assert!(rule.matches("a.b.c"));
        assert!(!rule.matches("b.c"));
        assert!(!rule.matches("d.c"));
    }

    #[test]
    fn rule_ipv4_and_port() {
        let rule = Rule::from_str("192.168.1.1:80").unwrap();
        assert!(rule.allows_ip("192.168.1.1".parse().unwrap()));
        assert!(!rule.allows_ip("192.168.1.2".parse().unwrap()));
        assert!(rule.allows_port(80));
        assert!(!rule.allows_port(443));
        assert!(!rule.allows_domain("example.com"));
    }

    #[test]
    fn rule_ipv6_and_port() {
        let rule = Rule::from_str("[2001:db8::1]:443").unwrap();
        assert!(rule.allows_ip(IpAddr::V6("2001:db8::1".parse().unwrap())));
        assert!(!rule.allows_ip(IpAddr::V6("2001:db8::2".parse().unwrap())));
        assert!(rule.allows_port(443));
        assert!(!rule.allows_port(80));
        assert!(!rule.allows_domain("example.com"));
    }

    #[test]
    fn rule_ipv4_range() {
        let rule = Rule::from_str("192.168.1.0/24").unwrap();
        assert!(rule.allows_ip("192.168.1.1".parse().unwrap()));
        assert!(rule.allows_ip("192.168.1.255".parse().unwrap()));
        assert!(!rule.allows_ip("192.168.2.1".parse().unwrap()));
        assert!(rule.allows_port(80));
        assert!(!rule.allows_domain("example.com"));
    }

    #[test]
    fn rule_ipv6_range() {
        let rule = Rule::from_str("2001:db8::1/32").unwrap();
        assert!(rule.allows_ip("2001:db8::1".parse().unwrap()));
        assert!(rule.allows_ip("2001:db8:0:0:0:0:0:1234".parse().unwrap()));
        assert!(!rule.allows_ip("2001:db7::1".parse().unwrap()));
        assert!(rule.allows_port(80));
        assert!(!rule.allows_domain("example.com"));
    }

    #[test]
    fn rule_domain_with_subdomains() {
        let rule = Rule::from_str("*.example.com").unwrap();
        assert!(rule.allows_domain("sub.example.com"));
        assert!(rule.allows_domain("another.sub.example.com"));
        assert!(!rule.allows_domain("example.com"));
        assert!(!rule.allows_domain("other.com"));
    }

    #[test]
    fn rule_all_domains_specific_port() {
        let rule = Rule::from_str("*.*:80").unwrap();
        assert!(rule.allows_domain("sub.example.com"));
        assert!(rule.allows_domain("another.sub.example.com"));
        assert!(rule.allows_domain("example.com"));
        assert!(rule.allows_domain("other.com"));
    }

    #[test]
    fn rule_all_ip_specific_port() {
        let rule = Rule::from_str("*:80-100").unwrap();
        assert!(rule.allows_ip("192.168.1.1".parse().unwrap()));
        assert!(rule.allows_ip("2001:db8::1".parse().unwrap()));
        assert!(rule.allows_port(80));
        assert!(!rule.allows_port(79));
        for port in 80..=100 {
            assert!(rule.allows_port(port));
        }
        assert!(!rule.allows_port(101));
    }

    #[test]
    fn negative_rule_ipv4_and_port() {
        let rule = Rule::from_str("!192.168.1.1:80").unwrap();

        assert!(!rule.allows_ip("192.168.1.1".parse().unwrap()));
        assert!(!rule.allows_ip("192.168.1.2".parse().unwrap()));
        assert!(!rule.allows_port(80));
        assert!(!rule.allows_port(443));
        assert!(!rule.allows_domain("example.com"));

        assert!(rule.blocks_ip("192.168.1.1".parse().unwrap()));
        assert!(!rule.blocks_ip("192.168.1.2".parse().unwrap()));
        assert!(rule.blocks_port(80));
        assert!(!rule.blocks_port(443));
        assert!(!rule.blocks_domain("example.com"));
    }

    #[test]
    fn negative_rule_ipv6_and_port() {
        let rule = Rule::from_str("![2001:db8::1]:443").unwrap();

        assert!(!rule.allows_ip(IpAddr::V6("2001:db8::1".parse().unwrap())));
        assert!(!rule.allows_ip(IpAddr::V6("2001:db8::2".parse().unwrap())));
        assert!(!rule.allows_port(443));
        assert!(!rule.allows_port(80));
        assert!(!rule.allows_domain("example.com"));

        assert!(rule.blocks_ip(IpAddr::V6("2001:db8::1".parse().unwrap())));
        assert!(!rule.blocks_ip(IpAddr::V6("2001:db8::2".parse().unwrap())));
        assert!(rule.blocks_port(443));
        assert!(!rule.blocks_port(80));
        assert!(!rule.blocks_domain("example.com"));
    }

    #[test]
    fn negative_rule_ipv4_range() {
        let rule = Rule::from_str("!192.168.1.0/24").unwrap();

        assert!(!rule.allows_ip("192.168.1.1".parse().unwrap()));
        assert!(!rule.allows_ip("192.168.1.255".parse().unwrap()));
        assert!(!rule.allows_ip("192.168.2.1".parse().unwrap()));
        assert!(!rule.allows_port(80));
        assert!(!rule.allows_domain("example.com"));

        assert!(rule.blocks_ip("192.168.1.1".parse().unwrap()));
        assert!(rule.blocks_ip("192.168.1.255".parse().unwrap()));
        assert!(!rule.blocks_ip("192.168.2.1".parse().unwrap()));
        assert!(rule.blocks_port(80));
        assert!(!rule.blocks_domain("example.com"));
    }

    #[test]
    fn negative_rule_ipv6_range() {
        let rule = Rule::from_str("!2001:db8::1/32").unwrap();

        assert!(!rule.allows_ip("2001:db8::1".parse().unwrap()));
        assert!(!rule.allows_ip("2001:db8:0:0:0:0:0:1234".parse().unwrap()));
        assert!(!rule.allows_ip("2001:db7::1".parse().unwrap()));
        assert!(!rule.allows_port(80));
        assert!(!rule.allows_domain("example.com"));

        assert!(rule.blocks_ip("2001:db8::1".parse().unwrap()));
        assert!(rule.blocks_ip("2001:db8:0:0:0:0:0:1234".parse().unwrap()));
        assert!(!rule.blocks_ip("2001:db7::1".parse().unwrap()));
        assert!(rule.blocks_port(80));
        assert!(!rule.blocks_domain("example.com"));
    }

    #[test]
    fn negative_rule_domain_with_subdomains() {
        let rule = Rule::from_str("!*.example.com").unwrap();

        assert!(!rule.allows_domain("sub.example.com"));
        assert!(!rule.allows_domain("another.sub.example.com"));
        assert!(!rule.allows_domain("example.com"));
        assert!(!rule.allows_domain("other.com"));

        assert!(rule.blocks_domain("sub.example.com"));
        assert!(rule.blocks_domain("another.sub.example.com"));
        assert!(!rule.blocks_domain("example.com"));
        assert!(!rule.blocks_domain("other.com"));
    }

    #[test]
    fn negative_rule_all_domains_specific_port() {
        let rule = Rule::from_str("!*.*:80").unwrap();

        assert!(!rule.allows_domain("sub.example.com"));
        assert!(!rule.allows_domain("another.sub.example.com"));
        assert!(!rule.allows_domain("example.com"));
        assert!(!rule.allows_domain("other.com"));

        assert!(rule.blocks_domain("sub.example.com"));
        assert!(rule.blocks_domain("another.sub.example.com"));
        assert!(rule.blocks_domain("example.com"));
        assert!(rule.blocks_domain("other.com"));
    }

    #[test]
    fn negative_rule_all_ip_specific_port() {
        let rule = Rule::from_str("!*:80-100").unwrap();

        assert!(!rule.allows_ip("192.168.1.1".parse().unwrap()));
        assert!(!rule.allows_ip("2001:db8::1".parse().unwrap()));
        assert!(!rule.allows_port(80));
        assert!(!rule.allows_port(79));
        for port in 80..=100 {
            assert!(!rule.allows_port(port));
        }
        assert!(!rule.allows_port(101));

        assert!(rule.blocks_ip("192.168.1.1".parse().unwrap()));
        assert!(rule.blocks_ip("2001:db8::1".parse().unwrap()));
        assert!(rule.blocks_port(80));
        assert!(!rule.blocks_port(79));
        for port in 80..=100 {
            assert!(rule.blocks_port(port));
        }
        assert!(!rule.blocks_port(101));
    }
}
