/// A [`Ruleset`] can be used to specify a whitelist and a blacklist in order to
/// control the inbound and outbound traffic of a network.
///
/// ## Rule Specification
/// Each rule can be expressed like:
/// ```text
/// <rule_kind>:<rule_action>=<rule_expr>
///
/// <rule_kind>: dns, ipv4, ipv6
///
/// <rule_action>: allow | deny
///
/// dns:
/// <rule_expr>:
/// {<domain_spec>}:{<port_spec>} (this will be expanded to an outbound IP rule)
/// <domain_spec>: domain | domain glob | *
///
/// ipv4:
/// <rule_expr>:
/// <ipv4_specs>:<port_specs>/<in|out>
/// <ipv4_specs>: <ipv4_spec> | {<ipv4_spec>,}
/// <ipv4_spec>: ipv4 | ipv4_range | *
///
/// ipv6:
/// <rule_expr>:
/// <ipv6_specs>:<port_specs>/<in|out>
/// <ipv6_specs>: <ipv6_spec> | {<ipv6_spec>,}
/// <ipv6_spec>: ipv6 | ipv6_range | *
///
/// <port_specs>: <port_spec> | {<port_specs>,}
/// <port_spec>: port | start_port-end_port | *
/// ```
///
/// The current implementation supports:
///
/// ### Whitelisting and Blacklisting
/// Each rule can be expressed as an `allow` (whitelist) or `deny` (blacklist). A socket or domain
/// is only accessible if at least one rule whitelists it and no rule blacklists it.
///
/// ### Directional Filtering
/// IP based rules can be either directional by specifying `/in` or `/out` postfixes to the rule,
/// or bidirectional which is the default setting for these rules.
///
/// ### Rule Combination
/// In order to prevent repetition, the parts before and after the `:` could hold multiple values.
/// For example:
/// ```text
/// ipv4:deny={127.0.0.1/24, 192.168.1.1/24}:{80, 443}
/// ```
/// This is equivalent to:
/// ```text
/// ipv4:deny=127.0.0.1/24:80,
/// ipv4:deny=127.0.0.1/24:443,
/// ipv4:deny=192.168.1.1/24:80,
/// ipv4:deny=192.168.1.1/24:443
/// ```
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::ops::RangeInclusive;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use ipnet::{Ipv4Net, Ipv6Net};
use iprange::IpRange;

/// Represents the errors that could happen during parsing the ruleset
#[derive(Debug, thiserror::Error)]
pub enum RuleParseError {
    #[error("invalid connection direction: {0}")]
    Direction(String),
    #[error("failed to parse int: {0}")]
    InvalidInteger(#[from] std::num::ParseIntError),
    #[error("failed to parse IP range address: {0}")]
    InvalidIpRange(#[from] ipnet::AddrParseError),
    #[error("failed to parse IP address: {0}")]
    InvalidIpAddr(#[from] std::net::AddrParseError),
    #[error("missing colon in rule: {0}")]
    MissingColon(String),
    #[error("Single IPV6 entry is not enclosed in brackets: {0}")]
    MalformedIpv6(String),
    #[error("Invalid rule type: {0}. Rule type must be either dns, ipv4, or ipv6")]
    InvalidRuleType(String),
    #[error("Invalid rule action: {0}. Rule action must be either allow or deny")]
    InvalidRuleAction(String),
    #[error("Domain rule not found for: {0}")]
    DomainRuleNotFound(String),
    #[error("Domain rule already expanded: {0}")]
    DomainAlreadyExpanded(String),
}

/// Represents the direction of the network traffic
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
    Bidirectional,
}

impl Direction {
    pub fn matches(&self, direction: Direction) -> bool {
        *self == Direction::Bidirectional || *self == direction
    }
}

impl FromStr for Direction {
    type Err = RuleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let direction = if s == "in" {
            Direction::Inbound
        } else if s == "out" {
            Direction::Outbound
        } else {
            return Err(RuleParseError::Direction(s.to_string()));
        };

        Ok(direction)
    }
}

/// Specification of a port rule
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortSpec {
    /// All ports are allowed
    All,
    /// Allows a single port
    Port(u16),
    /// Allows a range of ports
    PortRange(RangeInclusive<u16>),
}

impl PortSpec {
    pub fn matches(&self, port: u16) -> bool {
        match self {
            PortSpec::All => true,
            PortSpec::Port(allowed_port) => *allowed_port == port,
            PortSpec::PortRange(allowed_port_range) => allowed_port_range.contains(&port),
        }
    }
}

impl FromStr for PortSpec {
    type Err = RuleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let rule = if s == "*" {
            PortSpec::All
        } else if s.contains('-') {
            let (start, end) = s.split_once('-').unwrap();

            let (start, end) = (start.parse()?, end.parse()?);

            PortSpec::PortRange(start..=end)
        } else {
            PortSpec::Port(s.parse()?)
        };

        Ok(rule)
    }
}

/// Specification of a domain
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainSpec {
    /// All domains
    All,
    /// A single domain like: example.com
    Domain(String),
    /// A domain glob like: *.example.com
    DomainGlob(String),
}

impl DomainSpec {
    pub fn matches(&self, domain: impl AsRef<str>) -> bool {
        let domain = domain.as_ref();

        match self {
            DomainSpec::All => true,
            DomainSpec::Domain(allowed_domain) => allowed_domain == domain,
            DomainSpec::DomainGlob(domain_glob) => domain.ends_with(domain_glob),
        }
    }
}

impl FromStr for DomainSpec {
    type Err = RuleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let spec = if s == "*" {
            DomainSpec::All
        } else if let Some(glob) = s.strip_prefix('*') {
            DomainSpec::DomainGlob(glob.to_string())
        } else {
            DomainSpec::Domain(s.to_string())
        };

        Ok(spec)
    }
}

/// Represents a DNS rule
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DNSRule {
    // The allowed domain
    domain: DomainSpec,
    // The allowed port
    port: PortSpec,
    // Indicates whether this rule has been expanded into
    // a list of IP and port based rules
    expanded: bool,
}

impl DNSRule {
    /// Returns `true` if the `domain` is allowed by this rule
    pub fn allows(&self, domain: impl AsRef<str>) -> bool {
        self.domain.matches(domain)
    }

    /// Returns the allowed ports on the domains allowed by this rule
    pub fn allowed_ports(&self) -> PortSpec {
        self.port.clone()
    }
}

/// Specification of an Ipv4
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IPV4Spec {
    /// All IPs
    All,
    /// A single IP
    IP(Ipv4Addr),
    /// An IP range in the format of `ip/mask`
    IPRange(IpRange<Ipv4Net>),
}

impl IPV4Spec {
    pub fn matches(&self, ip: impl Into<Ipv4Addr>) -> bool {
        let ip = ip.into();

        match self {
            IPV4Spec::All => true,
            IPV4Spec::IP(allowed_ip) => *allowed_ip == ip,
            IPV4Spec::IPRange(allowed_ip_range) => allowed_ip_range.contains(&ip),
        }
    }
}

impl FromStr for IPV4Spec {
    type Err = RuleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let spec = if s == "*" {
            IPV4Spec::All
        } else if s.contains('/') {
            let ip = Ipv4Net::from_str(s)?;
            let mut ip_range = IpRange::<Ipv4Net>::new();
            ip_range.add(ip);

            IPV4Spec::IPRange(ip_range)
        } else {
            IPV4Spec::IP(s.parse()?)
        };

        Ok(spec)
    }
}

/// Represents an Ipv4 rule
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IPV4Rule {
    // Allowed IPs
    ip_spec: IPV4Spec,
    // Allowed ports
    port_spec: PortSpec,
    // Allowed direction of the traffic
    direction: Direction,
}

impl IPV4Rule {
    pub fn is_allowed(&self, ip: impl Into<Ipv4Addr>, port: u16, dir: Direction) -> bool {
        let ip = ip.into();

        self.ip_spec.matches(ip) && self.port_spec.matches(port) && self.direction.matches(dir)
    }
}

/// Specification of an Ipv6 address
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IPV6Spec {
    /// All IPs
    All,
    /// Single IP
    IP(Ipv6Addr),
    /// An IP range in the format of `ip/mask`
    IPRange(IpRange<Ipv6Net>),
}

impl IPV6Spec {
    pub fn matches(&self, ip: Ipv6Addr) -> bool {
        match self {
            IPV6Spec::All => true,
            IPV6Spec::IP(allowed_ip) => *allowed_ip == ip,
            IPV6Spec::IPRange(allowed_ip_range) => allowed_ip_range.contains(&ip),
        }
    }
}

impl FromStr for IPV6Spec {
    type Err = RuleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let spec = if s == "*" {
            IPV6Spec::All
        } else if s.contains('/') {
            let ip = Ipv6Net::from_str(s)?;
            let mut ip_range = IpRange::<Ipv6Net>::new();
            ip_range.add(ip);

            IPV6Spec::IPRange(ip_range)
        } else {
            IPV6Spec::IP(s.parse()?)
        };

        Ok(spec)
    }
}

/// Represents an Ipv6 rule
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IPV6Rule {
    // Allowed IPs
    ip_spec: IPV6Spec,
    // Allowed ports
    port_spec: PortSpec,
    // Allowed direction of the traffic
    direction: Direction,
}

impl IPV6Rule {
    pub fn is_allowed(&self, ip: impl Into<Ipv6Addr>, port: u16, dir: Direction) -> bool {
        let ip = ip.into();

        self.ip_spec.matches(ip) && self.port_spec.matches(port) && self.direction.matches(dir)
    }
}

/// Represents all supported rules
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rule {
    /// Allowed IPv4 traffic
    IPV4(IPV4Rule),
    /// Allowed IPv6 traffic
    IPV6(IPV6Rule),
    /// Allowed DNS queries
    DNS(DNSRule),
    /// Negative of a rule
    Neg(Arc<Rule>),
}

impl Rule {
    /// Returns `true` if this rule allows accessing `socket_addr` in the specific `direction`
    pub fn allows_socket(&self, socket_addr: SocketAddr, direction: Direction) -> bool {
        let ip = socket_addr.ip();
        let port = socket_addr.port();

        match (self, ip) {
            (Rule::IPV4(rule), IpAddr::V4(ip)) => rule.is_allowed(ip, port, direction),
            (Rule::IPV6(rule), IpAddr::V6(ip)) => rule.is_allowed(ip, port, direction),
            _ => false,
        }
    }

    /// Returns `true` if this rule allows querying the specific `domain`
    pub fn allows_domain(&self, domain: impl AsRef<str>) -> bool {
        if let Rule::DNS(rule) = self {
            rule.allows(domain)
        } else {
            false
        }
    }

    /// Returns `true` if this rule blocks accessing `socket_addr` in the specific `direction`
    pub fn blocks_socket(&self, socket_addr: SocketAddr, direction: Direction) -> bool {
        if let Rule::Neg(rule) = self {
            rule.allows_socket(socket_addr, direction)
        } else {
            false
        }
    }

    /// Returns `true` if this rule blocks querying the specific `domain`
    pub fn blocks_domain(&self, domain: impl AsRef<str>) -> bool {
        if let Rule::Neg(rule) = self {
            rule.allows_domain(domain)
        } else {
            false
        }
    }

    /// Returns allowed ports for the specified `domain` if this rule is a DNS rule
    pub fn port_spec_of_domain(&mut self, domain: impl AsRef<str>) -> Option<PortSpec> {
        if let Rule::DNS(rule) = self {
            if rule.allows(domain) {
                return Some(rule.allowed_ports());
            }
        }

        None
    }

    /// Returns `true` if this rule is a DNS rule and has not been expanded yet
    pub fn is_expandable(&self) -> bool {
        if let Rule::DNS(rule) = self {
            !rule.expanded
        } else {
            false
        }
    }

    /// Sets the expanded state of this rule if its a DNS rule
    pub fn set_expanded(&mut self, expanded: bool) {
        if let Rule::DNS(rule) = self {
            rule.expanded = expanded;
        }
    }
}

fn parse_enclosed(s: &str, left: char, right: char) -> Option<&str> {
    match (s.find(left), s.rfind(right)) {
        (Some(left_idx), Some(right_idx)) if left_idx < right_idx => {
            Some(&s[left_idx + 1..right_idx])
        }
        _ => None,
    }
}

fn parse_as_list<T: FromStr<Err = RuleParseError>>(s: &str) -> Result<Vec<T>, RuleParseError> {
    let entries = if let Some(entries) = parse_enclosed(s, '{', '}') {
        entries
            .split(',')
            .map(|s| s.trim().parse())
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let entry = T::from_str(s)?;

        vec![entry]
    };

    Ok(entries)
}

fn parse_ipv4_rule(s: &str) -> Result<Vec<IPV4Rule>, RuleParseError> {
    let (ips, ports_and_direction) = s
        .split_once(':')
        .ok_or_else(|| RuleParseError::MissingColon(s.to_string()))?;

    let mut direction = Direction::Bidirectional;
    let ports = if let Some((ports, dir)) = ports_and_direction.split_once('/') {
        direction = dir.parse()?;

        ports
    } else {
        ports_and_direction
    };

    let mut rules = Vec::new();
    let ips = parse_as_list::<IPV4Spec>(ips)?;
    let ports = parse_as_list::<PortSpec>(ports)?;

    for ip in &ips {
        for port in &ports {
            rules.push(IPV4Rule {
                ip_spec: ip.clone(),
                port_spec: port.clone(),
                direction,
            });
        }
    }

    Ok(rules)
}

fn parse_ipv6_rule(s: &str) -> Result<Vec<IPV6Rule>, RuleParseError> {
    let (ips, ports_and_direction) = s
        .rsplit_once(':')
        .ok_or_else(|| RuleParseError::MissingColon(s.to_string()))?;

    let mut direction = Direction::Bidirectional;
    let ports = if let Some((ports, dir)) = ports_and_direction.split_once('/') {
        direction = dir.parse()?;

        ports
    } else {
        ports_and_direction
    };

    let mut rules = Vec::new();

    let ips = if ips.contains('[') {
        let ip = parse_enclosed(ips, '[', ']')
            .ok_or_else(|| RuleParseError::MalformedIpv6(ips.to_string()))?;

        vec![ip.parse::<IPV6Spec>()?]
    } else {
        parse_as_list::<IPV6Spec>(ips)?
    };
    let ports = parse_as_list::<PortSpec>(ports)?;

    for ip in &ips {
        for port in &ports {
            rules.push(IPV6Rule {
                ip_spec: ip.clone(),
                port_spec: port.clone(),
                direction,
            });
        }
    }

    Ok(rules)
}

fn parse_dns_rule(s: &str) -> Result<Vec<DNSRule>, RuleParseError> {
    let (domains, ports) = s
        .split_once(':')
        .ok_or_else(|| RuleParseError::MissingColon(s.to_string()))?;

    let mut rules = Vec::new();
    let domains = parse_as_list::<DomainSpec>(domains)?;
    let ports = parse_as_list::<PortSpec>(ports)?;

    for domain in &domains {
        for port in &ports {
            rules.push(DNSRule {
                domain: domain.clone(),
                port: port.clone(),
                expanded: false,
            });
        }
    }

    Ok(rules)
}

// Represents the rule type section in a rule segment
#[derive(Debug, Clone, PartialEq, Eq)]
enum RuleType {
    Dns,
    IPV4,
    IPV6,
}

impl RuleType {
    // Receives a string as input and returns the parsed out rule type and the remaining string
    // |-------------|---...
    // rule_type ----^     ^
    // rem ----------------'
    pub fn consume_input(input: &str) -> Result<(Self, &str), RuleParseError> {
        let pair = if let Some(rem) = input.strip_prefix("dns:") {
            (RuleType::Dns, rem)
        } else if let Some(rem) = input.strip_prefix("ipv4:") {
            (RuleType::IPV4, rem)
        } else if let Some(rem) = input.strip_prefix("ipv6:") {
            (RuleType::IPV6, rem)
        } else {
            return Err(RuleParseError::InvalidRuleType(input.to_string()));
        };

        Ok(pair)
    }
}

// Represents the rule action section in a [`RulesetSegment`]
#[derive(Debug, Clone, PartialEq, Eq)]
enum RuleAction {
    Allow,
    Deny,
}

impl RuleAction {
    // Receives a string as input and returns the parsed out rule action and the remaining string
    // |----------|---------|---...
    // rule_type -^         ^     ^
    // rule_action ---------'     '
    // rem -----------------------'
    pub fn consume_input(input: &str) -> Result<(Self, &str), RuleParseError> {
        let pair = if let Some(rem) = input.strip_prefix("allow=") {
            (RuleAction::Allow, rem)
        } else if let Some(rem) = input.strip_prefix("deny=") {
            (RuleAction::Deny, rem)
        } else {
            return Err(RuleParseError::InvalidRuleAction(input.to_string()));
        };

        Ok(pair)
    }
}

// Represents the rule expression section in a [`RulesetSegment`]
#[derive(Debug, Clone, PartialEq, Eq)]
struct RuleExpr(String);

impl RuleExpr {
    // Receives a string as input and returns the parsed out rule expression and the remaining string
    // |----------|---------|-----|---...
    // rule_type -^         ^     ^     ^
    // rule_action ---------'     '     '
    // rule_expr -----------------'     '
    // rem -----------------------------'
    pub fn consume_input(input: &str) -> Result<(Self, &str), RuleParseError> {
        let mut next_dns_entry = usize::MAX;
        let mut next_ipv4_entry = usize::MAX;
        let mut next_ipv6_entry = usize::MAX;

        if let Some(idx) = input.find(",dns:") {
            next_dns_entry = idx;
        }

        if let Some(idx) = input.find(",ipv4:") {
            next_ipv4_entry = idx;
        }

        if let Some(idx) = input.find(",ipv6:") {
            next_ipv6_entry = idx;
        }

        let next_entry = next_dns_entry
            .min(next_ipv4_entry)
            .min(next_ipv6_entry)
            .min(input.len());

        let (expr, rem) = input.split_at(next_entry);

        let rem = rem.strip_prefix(',').unwrap_or(rem);

        Ok((RuleExpr(expr.to_string()), rem))
    }
}

// A ruleset is a series of comma separated ruleset segments:
//     <rule1>, <rule2>, ...
// each rule is consistent of three sections:
//     <rule-type>:<rule-action>=<rule-expr>
#[derive(Debug, Clone, PartialEq, Eq)]
struct RulesetSegment {
    ty: RuleType,
    action: RuleAction,
    expr: RuleExpr,
}

fn parse_ruleset_segments(s: impl AsRef<str>) -> Result<Vec<RulesetSegment>, RuleParseError> {
    let mut input = s.as_ref();
    let mut segments = Vec::new();

    while !input.is_empty() {
        let (ty, remaining) = RuleType::consume_input(input)?;
        let (action, remaining) = RuleAction::consume_input(remaining)?;
        let (expr, remaining) = RuleExpr::consume_input(remaining)?;

        segments.push(RulesetSegment { ty, action, expr });

        input = remaining;
    }

    Ok(segments)
}

/// Represents a ruleset that can be used to specify a whitelist and a blacklist in order to
/// control the inbound and outbound traffic of a network.
#[derive(Debug, Clone)]
pub struct Ruleset {
    rules: Arc<RwLock<Vec<Rule>>>,
}

impl Ruleset {
    /// Returns `true` if at least one rule allows accessing `socket_addr` in the specific `direction`
    /// and no rule blocks it
    pub fn allows_socket(&self, addr: impl Into<SocketAddr>, dir: Direction) -> bool {
        let addr = addr.into();

        let is_allowed = {
            let ruleset = self.rules.read().unwrap();

            let is_blacklisted = ruleset.iter().any(|r| r.blocks_socket(addr, dir));
            if is_blacklisted {
                return false;
            }

            ruleset.iter().any(|r| r.allows_socket(addr, dir))
        };

        is_allowed
    }

    /// Returns `true` if at least one rule allows querying the specific `domain` and no rule blocks it
    pub fn allows_domain(&self, domain: impl AsRef<str>) -> bool {
        let domain = domain.as_ref();

        let is_allowed = {
            let ruleset = self.rules.read().unwrap();

            let is_blacklisted = ruleset.iter().any(|r| r.blocks_domain(domain));
            if is_blacklisted {
                return false;
            }

            ruleset.iter().any(|r| r.allows_domain(domain))
        };

        is_allowed
    }

    /// Expands the DNS rule that allows the specified `domain` into a list of IP based
    /// rules with addresses specified by `addrs`
    pub fn expand_domain(
        &self,
        domain: impl AsRef<str>,
        addrs: impl AsRef<[IpAddr]>,
    ) -> Result<(), RuleParseError> {
        let mut ruleset = self.rules.write().unwrap();
        let domain = domain.as_ref();

        let mut already_expanded = false;
        let port_spec = ruleset
            .iter_mut()
            .find_map(|rule| {
                let port_spec = rule.port_spec_of_domain(domain);

                if port_spec.is_some() {
                    if rule.is_expandable() {
                        rule.set_expanded(true);

                        return port_spec;
                    } else {
                        already_expanded = true;
                    }
                }

                None
            })
            .ok_or_else(|| {
                if already_expanded {
                    RuleParseError::DomainAlreadyExpanded(domain.to_string())
                } else {
                    RuleParseError::DomainRuleNotFound(domain.to_string())
                }
            })?;

        for addr in addrs.as_ref() {
            let rule = match addr {
                IpAddr::V4(ip) => Rule::IPV4(IPV4Rule {
                    ip_spec: IPV4Spec::IP(*ip),
                    port_spec: port_spec.clone(),
                    direction: Direction::Outbound,
                }),
                IpAddr::V6(ip) => Rule::IPV6(IPV6Rule {
                    ip_spec: IPV6Spec::IP(*ip),
                    port_spec: port_spec.clone(),
                    direction: Direction::Outbound,
                }),
            };

            ruleset.push(rule);
        }

        Ok(())
    }
}

impl FromStr for Ruleset {
    type Err = RuleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        let mut rules = vec![];
        for seg in parse_ruleset_segments(s)? {
            let rule_type = &seg.ty;
            let rule_action = &seg.action;
            let rule_expr = &seg.expr;

            let parsed_rules: Vec<Rule> = match rule_type {
                RuleType::Dns => parse_dns_rule(&rule_expr.0)?
                    .into_iter()
                    .map(Rule::DNS)
                    .collect(),
                RuleType::IPV4 => parse_ipv4_rule(&rule_expr.0)?
                    .into_iter()
                    .map(Rule::IPV4)
                    .collect(),
                RuleType::IPV6 => parse_ipv6_rule(&rule_expr.0)?
                    .into_iter()
                    .map(Rule::IPV6)
                    .collect(),
            };

            let parsed_rules = match rule_action {
                RuleAction::Allow => parsed_rules,
                RuleAction::Deny => parsed_rules
                    .into_iter()
                    .map(|rule| Rule::Neg(Arc::new(rule)))
                    .collect(),
            };

            rules.extend(parsed_rules);
        }

        Ok(Self {
            rules: Arc::new(RwLock::new(rules)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_ports_spec() {
        let spec = PortSpec::from_str("*").unwrap();

        assert!(spec.matches(80));
    }

    #[test]
    fn port_spec() {
        let spec = PortSpec::from_str("80").unwrap();

        assert!(spec.matches(80));
        assert!(!spec.matches(443));
    }

    #[test]
    fn port_range_spec() {
        let spec = PortSpec::from_str("80-85").unwrap();

        assert!(!spec.matches(79));
        assert!(spec.matches(80));
        assert!(spec.matches(81));
        assert!(spec.matches(82));
        assert!(spec.matches(83));
        assert!(spec.matches(84));
        assert!(spec.matches(85));
        assert!(!spec.matches(86));
    }

    #[test]
    fn all_domains_spec() {
        let spec = DomainSpec::from_str("*").unwrap();

        assert!(spec.matches("example.com"));
    }

    #[test]
    fn domain_spec() {
        let spec = DomainSpec::from_str("example.com").unwrap();

        assert!(spec.matches("example.com"));
        assert!(!spec.matches("sub.example.com"));
        assert!(!spec.matches("test.com"));
    }

    #[test]
    fn domain_glob_spec() {
        let spec = DomainSpec::from_str("*.example.com").unwrap();

        assert!(!spec.matches("example.com"));
        assert!(spec.matches("sub.example.com"));
        assert!(spec.matches("another.sub.example.com"));
        assert!(!spec.matches("test.com"));
    }

    #[test]
    fn all_ipv4s_spec() {
        let spec = IPV4Spec::from_str("*").unwrap();

        assert!(spec.matches([127, 0, 0, 1]));
    }

    #[test]
    fn ipv4_spec() {
        let spec = IPV4Spec::from_str("127.0.0.1").unwrap();

        assert!(spec.matches([127, 0, 0, 1]));
        assert!(!spec.matches([192, 168, 1, 1]));
    }

    #[test]
    fn ipv4_range_spec() {
        let rule = IPV4Spec::from_str("192.168.1.0/24").unwrap();

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
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(rule.matches(ip_addr));
        }

        for ip in non_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!rule.matches(ip_addr));
        }
    }

    #[test]
    fn all_ipv6s_spec() {
        let spec = IPV6Spec::from_str("*").unwrap();

        assert!(spec.matches("2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn ipv6_spec() {
        let spec = IPV6Spec::from_str("2001:db8::1").unwrap();

        assert!(spec.matches("2001:db8::1".parse().unwrap()));
        assert!(!spec.matches("2001:db7::1".parse().unwrap()));
    }

    #[test]
    fn ipv6_range_spec() {
        let spec = IPV6Spec::from_str("2001:db8::/32").unwrap();

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
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(spec.matches(ip_addr));
        }

        for ip in non_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(!spec.matches(ip_addr));
        }
    }

    #[test]
    fn dns_rule_all() {
        let rules = parse_dns_rule("*:*").unwrap();

        assert_eq!(rules.len(), 1);
        assert!(rules[0].allows("example.com"));
        assert_eq!(rules[0].allowed_ports(), PortSpec::All);
    }

    #[test]
    fn dns_rule_single_domain_and_port() {
        let rules = parse_dns_rule("example.com:80").unwrap();

        assert_eq!(rules.len(), 1);
        assert!(rules[0].allows("example.com"));
        assert_eq!(rules[0].allowed_ports(), PortSpec::Port(80));
    }

    #[test]
    fn dns_rule_multiple_domain_and_ports() {
        let mut rules = parse_dns_rule("{a.com, *.b.com}:{80, 100-200}").unwrap();

        let rule1 = rules.pop().unwrap(); // *.b.com:100-200
        let rule2 = rules.pop().unwrap(); // *.b.com:80
        let rule3 = rules.pop().unwrap(); // a.com:100-200
        let rule4 = rules.pop().unwrap(); // a.com:80

        assert!(rules.is_empty());

        assert!(rule1.allows("sub.b.com"));
        assert!(!rule1.allows("b.com"));
        assert!(!rule1.allows("a.com"));
        assert_eq!(rule1.allowed_ports(), PortSpec::PortRange(100..=200));

        assert!(rule2.allows("sub.b.com"));
        assert!(!rule2.allows("b.com"));
        assert!(!rule2.allows("a.com"));
        assert_eq!(rule2.allowed_ports(), PortSpec::Port(80));

        assert!(rule3.allows("a.com"));
        assert!(!rule3.allows("sub.a.com"));
        assert!(!rule3.allows("b.com"));
        assert_eq!(rule3.allowed_ports(), PortSpec::PortRange(100..=200));

        assert!(rule4.allows("a.com"));
        assert!(!rule4.allows("sub.a.com"));
        assert!(!rule4.allows("b.com"));
        assert_eq!(rule4.allowed_ports(), PortSpec::Port(80));
    }

    #[test]
    fn ipv4_rule_all() {
        let rules = parse_ipv4_rule("*:*").unwrap();

        assert_eq!(rules.len(), 1);
        assert!(rules[0].is_allowed([127, 0, 0, 1], 80, Direction::Inbound));
        assert!(rules[0].is_allowed([127, 0, 0, 1], 80, Direction::Outbound));
    }

    #[test]
    fn ipv4_rule_single_ip_all_ports_inbound() {
        let rules = parse_ipv4_rule("127.0.0.1:*/in").unwrap();

        assert_eq!(rules.len(), 1);
        assert!(rules[0].is_allowed([127, 0, 0, 1], 80, Direction::Inbound));
        assert!(!rules[0].is_allowed([127, 0, 0, 1], 80, Direction::Outbound));
        assert!(!rules[0].is_allowed([192, 168, 1, 2], 80, Direction::Inbound));
        assert!(!rules[0].is_allowed([192, 168, 1, 2], 80, Direction::Outbound));
    }

    #[test]
    fn ipv4_rule_ip_range_all_ports_outbound() {
        let mut rules = parse_ipv4_rule("192.168.1.0/24:*/out").unwrap();

        let ip_matches = vec![
            "192.168.1.1",
            "192.168.1.0",
            "192.168.1.255",
            "192.168.1.100",
            "192.168.1.50",
        ];

        let ip_non_matches = vec![
            "192.168.2.0",
            "192.167.1.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.0.255",
        ];

        assert_eq!(rules.len(), 1);
        let rule = rules.pop().unwrap();

        for ip in &ip_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(rule.is_allowed(ip_addr, 8080, Direction::Outbound));
        }
        // direction is wrong
        for ip in &ip_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!rule.is_allowed(ip_addr, 8080, Direction::Inbound));
        }
        // ip is wrong
        for ip in &ip_non_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!rule.is_allowed(ip_addr, 8080, Direction::Inbound));
        }
    }

    #[test]
    fn ipv4_rule_all_ip_port_range_outbound() {
        let rules = parse_ipv4_rule("*:80-90/out").unwrap();

        assert_eq!(rules.len(), 1);
        assert!(!rules[0].is_allowed([127, 0, 0, 1], 80, Direction::Inbound));
        assert!(rules[0].is_allowed([127, 0, 0, 1], 80, Direction::Outbound));
        assert!(rules[0].is_allowed([127, 0, 0, 1], 85, Direction::Outbound));
        assert!(rules[0].is_allowed([127, 0, 0, 1], 90, Direction::Outbound));
        assert!(!rules[0].is_allowed([127, 0, 0, 1], 443, Direction::Outbound));
        assert!(!rules[0].is_allowed([192, 168, 1, 2], 80, Direction::Inbound));
        assert!(rules[0].is_allowed([192, 168, 1, 2], 80, Direction::Outbound));
    }

    #[test]
    fn multiple_ipv4_rules() {
        let mut rules = parse_ipv4_rule("{127.0.0.1, 192.168.1.0/24}:{80, 8080}/in").unwrap();

        let rule1 = rules.pop().unwrap(); // 192.168.1.0/24:8080/in
        let rule2 = rules.pop().unwrap(); // 192.168.1.0/24:80/in
        let rule3 = rules.pop().unwrap(); // 127.0.0.1:8080/in
        let rule4 = rules.pop().unwrap(); // 127.0.0.1:80/in

        assert!(rules.is_empty());

        let ip_matches = vec![
            "192.168.1.1",
            "192.168.1.0",
            "192.168.1.255",
            "192.168.1.100",
            "192.168.1.50",
        ];

        let ip_non_matches = vec![
            "192.168.2.0",
            "192.167.1.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.0.255",
        ];

        // rule1
        for ip in &ip_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(rule1.is_allowed(ip_addr, 8080, Direction::Inbound));
        }
        // direction is wrong
        for ip in &ip_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!rule1.is_allowed(ip_addr, 8080, Direction::Outbound));
        }
        // port is wrong
        for ip in &ip_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!rule1.is_allowed(ip_addr, 80, Direction::Inbound));
        }
        // ip is wrong
        for ip in &ip_non_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!rule1.is_allowed(ip_addr, 8080, Direction::Inbound));
        }

        // rule2
        for ip in &ip_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(rule2.is_allowed(ip_addr, 80, Direction::Inbound));
        }
        // direction is wrong
        for ip in &ip_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!rule2.is_allowed(ip_addr, 80, Direction::Outbound));
        }
        // port is wrong
        for ip in &ip_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!rule2.is_allowed(ip_addr, 8080, Direction::Inbound));
        }
        // ip is wrong
        for ip in &ip_non_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!rule2.is_allowed(ip_addr, 80, Direction::Inbound));
        }

        // rule3
        assert!(rule3.is_allowed([127, 0, 0, 1], 8080, Direction::Inbound));
        assert!(!rule3.is_allowed([192, 168, 1, 2], 8080, Direction::Inbound));
        assert!(!rule3.is_allowed([127, 0, 0, 1], 80, Direction::Inbound));
        assert!(!rule3.is_allowed([127, 0, 0, 1], 8080, Direction::Outbound));

        // rule4
        assert!(rule4.is_allowed([127, 0, 0, 1], 80, Direction::Inbound));
        assert!(!rule4.is_allowed([192, 168, 1, 2], 80, Direction::Inbound));
        assert!(!rule4.is_allowed([127, 0, 0, 1], 8080, Direction::Inbound));
        assert!(!rule4.is_allowed([127, 0, 0, 1], 80, Direction::Outbound));
    }

    #[test]
    fn ipv6_rule_all() {
        let rules = parse_ipv6_rule("*:*").unwrap();

        assert_eq!(rules.len(), 1);
        assert!(rules[0].is_allowed(
            "2001:db8::1".parse::<Ipv6Addr>().unwrap(),
            80,
            Direction::Inbound
        ));
        assert!(rules[0].is_allowed(
            "2001:db8::1".parse::<Ipv6Addr>().unwrap(),
            80,
            Direction::Outbound
        ));
    }

    #[test]
    fn ipv6_rule_single_ip_and_port() {
        let rules = parse_ipv6_rule("[2001:db8::1]:80").unwrap();

        assert_eq!(rules.len(), 1);
        assert!(rules[0].is_allowed(
            "2001:db8::1".parse::<Ipv6Addr>().unwrap(),
            80,
            Direction::Inbound
        ));
        assert!(rules[0].is_allowed(
            "2001:db8::1".parse::<Ipv6Addr>().unwrap(),
            80,
            Direction::Outbound
        ));
    }

    #[test]
    fn ipv6_rule_single_ip_all_ports_inbound() {
        let rules = parse_ipv6_rule("[2001:db8::1]:*/in").unwrap();

        assert_eq!(rules.len(), 1);
        assert!(rules[0].is_allowed(
            "2001:db8::1".parse::<Ipv6Addr>().unwrap(),
            80,
            Direction::Inbound
        ));
        assert!(!rules[0].is_allowed(
            "2002:db8::1".parse::<Ipv6Addr>().unwrap(),
            80,
            Direction::Inbound
        ));
        assert!(!rules[0].is_allowed(
            "2001:db8::1".parse::<Ipv6Addr>().unwrap(),
            8080,
            Direction::Outbound
        ));
    }

    #[test]
    fn ipv6_rule_ip_range_all_ports_outbound() {
        let mut rules = parse_ipv6_rule("[2001:db8::/32]:*/out").unwrap();

        let ip_matches = vec![
            "2001:db8::1",
            "2001:db8::",
            "2001:db8:0:0:0:0:0:1234",
            "2001:db8::abcd",
            "2001:db8::ffff",
        ];

        let ip_non_matches = vec![
            "2001:db9::",
            "2001:db7::1",
            "2001:dead::1",
            "fe80::1",
            "::1",
        ];

        assert_eq!(rules.len(), 1);
        let rule = rules.pop().unwrap();

        for ip in &ip_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(rule.is_allowed(ip_addr, 8080, Direction::Outbound));
        }
        // direction is wrong
        for ip in &ip_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(!rule.is_allowed(ip_addr, 8080, Direction::Inbound));
        }
        // ip is wrong
        for ip in &ip_non_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(!rule.is_allowed(ip_addr, 8080, Direction::Inbound));
        }
    }

    #[test]
    fn multiple_ipv6_rules() {
        let mut rules = parse_ipv6_rule("{3001:db8::, 2001:db8::/32}:{80, 8080}/in").unwrap();

        let rule1 = rules.pop().unwrap(); // [2001:db8::/32]:8080/in
        let rule2 = rules.pop().unwrap(); // [2001:db8::/32]:80/in
        let rule3 = rules.pop().unwrap(); // [3001:db8::]:8080/in
        let rule4 = rules.pop().unwrap(); // [3001:db8::]:80/in

        assert!(rules.is_empty());

        let ip_matches = vec![
            "2001:db8::1",
            "2001:db8::",
            "2001:db8:0:0:0:0:0:1234",
            "2001:db8::abcd",
            "2001:db8::ffff",
        ];

        let ip_non_matches = vec![
            "2001:db9::",
            "2001:db7::1",
            "2001:dead::1",
            "fe80::1",
            "::1",
        ];

        // rule1
        for ip in &ip_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(rule1.is_allowed(ip_addr, 8080, Direction::Inbound));
        }
        // direction is wrong
        for ip in &ip_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(!rule1.is_allowed(ip_addr, 8080, Direction::Outbound));
        }
        // port is wrong
        for ip in &ip_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(!rule1.is_allowed(ip_addr, 80, Direction::Inbound));
        }
        // ip is wrong
        for ip in &ip_non_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(!rule1.is_allowed(ip_addr, 8080, Direction::Inbound));
        }

        // rule2
        for ip in &ip_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(rule2.is_allowed(ip_addr, 80, Direction::Inbound));
        }
        // direction is wrong
        for ip in &ip_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(!rule2.is_allowed(ip_addr, 80, Direction::Outbound));
        }
        // port is wrong
        for ip in &ip_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(!rule2.is_allowed(ip_addr, 8080, Direction::Inbound));
        }
        // ip is wrong
        for ip in &ip_non_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(!rule2.is_allowed(ip_addr, 80, Direction::Inbound));
        }

        // rule3
        assert!(rule3.is_allowed(
            "3001:db8::".parse::<Ipv6Addr>().unwrap(),
            8080,
            Direction::Inbound
        ));
        assert!(!rule3.is_allowed(
            "4001:db8::".parse::<Ipv6Addr>().unwrap(),
            8080,
            Direction::Inbound
        ));
        assert!(!rule3.is_allowed(
            "3001:db8::".parse::<Ipv6Addr>().unwrap(),
            80,
            Direction::Inbound
        ));
        assert!(!rule3.is_allowed(
            "3001:db8::".parse::<Ipv6Addr>().unwrap(),
            8080,
            Direction::Outbound
        ));

        // rule4
        assert!(rule4.is_allowed(
            "3001:db8::".parse::<Ipv6Addr>().unwrap(),
            80,
            Direction::Inbound
        ));
        assert!(!rule4.is_allowed(
            "4001:db8::".parse::<Ipv6Addr>().unwrap(),
            80,
            Direction::Inbound
        ));
        assert!(!rule4.is_allowed(
            "3001:db8::".parse::<Ipv6Addr>().unwrap(),
            8080,
            Direction::Inbound
        ));
        assert!(!rule4.is_allowed(
            "3001:db8::".parse::<Ipv6Addr>().unwrap(),
            80,
            Direction::Outbound
        ));
    }

    #[test]
    fn ruleset_dns() {
        let ruleset = Ruleset::from_str("dns:allow={a.com, *.b.com}:{80, 8080}").unwrap();

        assert!(ruleset.allows_domain("a.com"));
        assert!(!ruleset.allows_domain("sub.a.com"));
        assert!(!ruleset.allows_domain("b.com"));
        assert!(ruleset.allows_domain("sub.b.com"));
        assert!(ruleset.allows_domain("another.sub.b.com"));
    }

    #[test]
    fn ruleset_ipv4() {
        let ruleset =
            Ruleset::from_str("ipv4:deny={127.0.0.1, 192.168.1.0/24}:{80, 8080}/in").unwrap();

        let ip_matches = vec![
            "192.168.1.1",
            "192.168.1.0",
            "192.168.1.255",
            "192.168.1.100",
            "192.168.1.50",
        ];

        for ip in &ip_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!ruleset.allows_socket((ip_addr, 8080), Direction::Inbound));
        }

        assert!(!ruleset.allows_socket(([127, 0, 0, 1], 8080), Direction::Inbound));
        assert!(!ruleset.allows_socket(([127, 0, 0, 1], 80), Direction::Inbound));
    }

    #[test]
    fn ruleset_ipv6() {
        let ruleset =
            Ruleset::from_str("ipv6:allow={3001:db8::, 2001:db8::/32}:{80, 8080}/in").unwrap();

        let ip_matches = vec![
            "2001:db8::1",
            "2001:db8::",
            "2001:db8:0:0:0:0:0:1234",
            "2001:db8::abcd",
            "2001:db8::ffff",
        ];

        for ip in &ip_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(ruleset.allows_socket((ip_addr, 8080), Direction::Inbound));
        }

        assert!(ruleset.allows_socket(
            ("3001:db8::".parse::<Ipv6Addr>().unwrap(), 8080),
            Direction::Inbound
        ));
        assert!(ruleset.allows_socket(
            ("3001:db8::".parse::<Ipv6Addr>().unwrap(), 8080),
            Direction::Inbound
        ));
    }

    #[test]
    fn ruleset_full() {
        let ruleset = Ruleset::from_str(
            "dns:allow={a.com, *.b.com}:{80, 8080},
            ipv4:deny={127.0.0.1, 192.168.1.0/24}:{80, 8080}/in,
            ipv6:allow={3001:db8::, 2001:db8::/32}:{80, 8080}/in",
        )
        .unwrap();

        // dns rules
        assert!(ruleset.allows_domain("a.com"));
        assert!(!ruleset.allows_domain("sub.a.com"));
        assert!(!ruleset.allows_domain("b.com"));
        assert!(ruleset.allows_domain("sub.b.com"));
        assert!(ruleset.allows_domain("another.sub.b.com"));

        // ipv4 rules
        let ip_matches = vec![
            "192.168.1.1",
            "192.168.1.0",
            "192.168.1.255",
            "192.168.1.100",
            "192.168.1.50",
        ];

        for ip in &ip_matches {
            let ip_addr: Ipv4Addr = ip.parse().unwrap();
            assert!(!ruleset.allows_socket((ip_addr, 8080), Direction::Inbound));
        }

        assert!(!ruleset.allows_socket(([127, 0, 0, 1], 8080), Direction::Inbound));
        assert!(!ruleset.allows_socket(([127, 0, 0, 1], 80), Direction::Inbound));

        // ipv6 rules
        let ip_matches = vec![
            "2001:db8::1",
            "2001:db8::",
            "2001:db8:0:0:0:0:0:1234",
            "2001:db8::abcd",
            "2001:db8::ffff",
        ];

        for ip in &ip_matches {
            let ip_addr: Ipv6Addr = ip.parse().unwrap();
            assert!(ruleset.allows_socket((ip_addr, 8080), Direction::Inbound));
        }

        assert!(ruleset.allows_socket(
            ("3001:db8::".parse::<Ipv6Addr>().unwrap(), 8080),
            Direction::Inbound
        ));
        assert!(ruleset.allows_socket(
            ("3001:db8::".parse::<Ipv6Addr>().unwrap(), 8080),
            Direction::Inbound
        ));
    }
}
