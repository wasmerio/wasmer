//! [`GlobalId`]s are used by the backend to identify a specific object.
//!
//! This module provides a parser/encoder and related type defintions
//! for global ids.

use std::fmt::Display;

#[repr(u16)]
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    User = 0,
    SocialAuth = 1,
    Namespace = 2,
    Package = 3,
    PackageVersion = 4,
    PackageCollaborator = 5,
    PackageCollaboratorInvite = 6,
    NativeExecutable = 7,
    PackageVersionNPMBinding = 8,
    PackageVersionPythonBinding = 9,
    PackageTransferRequest = 10,
    Interface = 11,
    InterfaceVersion = 12,
    PublicKey = 13,
    UserNotification = 14,
    ActivityEvent = 15,
    NamespaceCollaborator = 16,
    NamespaceCollaboratorInvite = 17,
    BindingsGenerator = 18,
    DeployConfigVersion = 19,
    DeployConfigInfo = 20,
    DeployApp = 21,
    DeployAppVersion = 22,
    Waitlist = 23,
    WaitlistMember = 24,
    CardPaymentMethod = 25,
    PaymentIntent = 26,
    AppAlias = 27,
    Nonce = 28,
    TermsOfService = 29,
}

impl NodeKind {
    pub fn from_num(x: u64) -> Option<Self> {
        match x {
            0 => Some(Self::User),
            1 => Some(Self::SocialAuth),
            2 => Some(Self::Namespace),
            3 => Some(Self::Package),
            4 => Some(Self::PackageVersion),
            5 => Some(Self::PackageCollaborator),
            6 => Some(Self::PackageCollaboratorInvite),
            7 => Some(Self::NativeExecutable),
            8 => Some(Self::PackageVersionNPMBinding),
            9 => Some(Self::PackageVersionPythonBinding),
            10 => Some(Self::PackageTransferRequest),
            11 => Some(Self::Interface),
            12 => Some(Self::InterfaceVersion),
            13 => Some(Self::PublicKey),
            14 => Some(Self::UserNotification),
            15 => Some(Self::ActivityEvent),
            16 => Some(Self::NamespaceCollaborator),
            17 => Some(Self::NamespaceCollaboratorInvite),
            18 => Some(Self::BindingsGenerator),
            19 => Some(Self::DeployConfigVersion),
            20 => Some(Self::DeployConfigInfo),
            21 => Some(Self::DeployApp),
            22 => Some(Self::DeployAppVersion),
            23 => Some(Self::Waitlist),
            24 => Some(Self::WaitlistMember),
            25 => Some(Self::CardPaymentMethod),
            26 => Some(Self::PaymentIntent),
            27 => Some(Self::AppAlias),
            28 => Some(Self::Nonce),
            29 => Some(Self::TermsOfService),
            _ => None,
        }
    }

    pub fn parse_prefix(s: &str) -> Option<NodeKind> {
        match s {
            "u" => Some(Self::User),
            "su" => Some(Self::SocialAuth),
            "ns" => Some(Self::Namespace),
            "pk" => Some(Self::Package),
            "pkv" => Some(Self::PackageVersion),
            "pc" => Some(Self::PackageCollaborator),
            "pci" => Some(Self::PackageCollaboratorInvite),
            "ne" => Some(Self::NativeExecutable),
            "pkvbjs" => Some(Self::PackageVersionNPMBinding),
            "pkvbpy" => Some(Self::PackageVersionPythonBinding),
            "pt" => Some(Self::PackageTransferRequest),
            "in" => Some(Self::Interface),
            "inv" => Some(Self::InterfaceVersion),
            "pub" => Some(Self::PublicKey),
            "nt" => Some(Self::UserNotification),
            "ae" => Some(Self::ActivityEvent),
            "nsc" => Some(Self::NamespaceCollaborator),
            "nsci" => Some(Self::NamespaceCollaboratorInvite),
            "bg" => Some(Self::BindingsGenerator),
            "dcv" => Some(Self::DeployConfigVersion),
            "dci" => Some(Self::DeployConfigInfo),
            "da" => Some(Self::DeployApp),
            "dav" => Some(Self::DeployAppVersion),
            "wl" => Some(Self::Waitlist),
            "wlm" => Some(Self::WaitlistMember),
            "cpm" => Some(Self::CardPaymentMethod),
            "pi" => Some(Self::PaymentIntent),
            "daa" => Some(Self::AppAlias),
            "nnc" => Some(Self::Nonce),
            "tos" => Some(Self::TermsOfService),
            _ => None,
        }
    }

    fn as_prefix(&self) -> &'static str {
        match self {
            Self::User => "u",
            Self::SocialAuth => "su",
            Self::Namespace => "ns",
            Self::Package => "pk",
            Self::PackageVersion => "pkv",
            Self::PackageCollaborator => "pc",
            Self::PackageCollaboratorInvite => "pci",
            Self::NativeExecutable => "ne",
            Self::PackageVersionNPMBinding => "pkvbjs",
            Self::PackageVersionPythonBinding => "pkvbpy",
            Self::PackageTransferRequest => "pt",
            Self::Interface => "in",
            Self::InterfaceVersion => "inv",
            Self::PublicKey => "pub",
            Self::UserNotification => "nt",
            Self::ActivityEvent => "ae",
            Self::NamespaceCollaborator => "nsc",
            Self::NamespaceCollaboratorInvite => "nsci",
            Self::BindingsGenerator => "bg",
            Self::DeployConfigVersion => "dcv",
            Self::DeployConfigInfo => "dci",
            Self::DeployApp => "da",
            Self::DeployAppVersion => "dav",
            Self::Waitlist => "wl",
            Self::WaitlistMember => "wlm",
            Self::CardPaymentMethod => "cpm",
            Self::PaymentIntent => "pi",
            Self::AppAlias => "daa",
            Self::Nonce => "nnc",
            Self::TermsOfService => "tos",
        }
    }
}

impl Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::User => "User",
            Self::SocialAuth => "SocialAuth",
            Self::Namespace => "Namespace",
            Self::Package => "Package",
            Self::PackageVersion => "PackageVersion",
            Self::PackageCollaborator => "PackageCollaborator",
            Self::PackageCollaboratorInvite => "PackageCollaboratorInvite",
            Self::NativeExecutable => "NativeExecutable",
            Self::PackageVersionNPMBinding => "PackageVersionNPMBinding",
            Self::PackageVersionPythonBinding => "PackageVersionPythonBinding",
            Self::PackageTransferRequest => "PackageTransferRequest",
            Self::Interface => "Interface",
            Self::InterfaceVersion => "InterfaceVersion",
            Self::PublicKey => "PublicKey",
            Self::UserNotification => "UserNotification",
            Self::ActivityEvent => "ActivityEvent",
            Self::NamespaceCollaborator => "NamespaceCollaborator",
            Self::NamespaceCollaboratorInvite => "NamespaceCollaboratorInvite",
            Self::BindingsGenerator => "BindingsGenerator",
            Self::DeployConfigVersion => "DeployConfigVersion",
            Self::DeployConfigInfo => "DeployConfigInfo",
            Self::DeployApp => "DeployApp",
            Self::DeployAppVersion => "DeployAppVersion",
            Self::Waitlist => "Waitlist",
            Self::WaitlistMember => "WaitlistMember",
            Self::CardPaymentMethod => "CardPaymentMethod",
            Self::PaymentIntent => "PaymentIntent",
            Self::AppAlias => "AppAlias",
            Self::Nonce => "Nonce",
            Self::TermsOfService => "TermsOfService",
        };
        write!(f, "{name}")
    }
}

/// Global id of backend nodes.
///
/// IDs are encoded using the "hashid" scheme, which uses a given alphabet and
/// a salt to encode u64 numbers into a string hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalId {
    /// The node type of the ID.
    kind: NodeKind,
    /// The database ID of the node.
    database_id: u64,
}

impl GlobalId {
    /// Salt used by the backend to encode hashes.
    const SALT: &'static str = "wasmer salt hashid";
    /// Minimum length of the encoded hashes.
    const MIN_LENGTH: usize = 12;

    /// Hash alphabet used for the prefix id variant.
    const ALPHABET_PREFIXED: &'static str =
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890";

    /// Hash alphabet used for the non-prefixed id variant.
    const ALPHABET_URL: &'static str = "abcdefghijklmnopqrstuvwxyz0123456789";

    pub fn new(kind: NodeKind, database_id: u64) -> Self {
        Self { kind, database_id }
    }

    fn build_harsh(alphabet: &str, salt: &[u8]) -> harsh::Harsh {
        harsh::HarshBuilder::new()
            .alphabet(alphabet.as_bytes())
            .salt(salt)
            .length(GlobalId::MIN_LENGTH)
            .build()
            .unwrap()
    }

    fn build_harsh_prefixed() -> harsh::Harsh {
        Self::build_harsh(Self::ALPHABET_PREFIXED, Self::SALT.as_bytes())
    }

    fn build_harsh_url() -> harsh::Harsh {
        Self::build_harsh(Self::ALPHABET_URL, Self::SALT.as_bytes())
    }

    pub fn kind(&self) -> NodeKind {
        self.kind
    }

    pub fn database_id(&self) -> u64 {
        self.database_id
    }

    /// Encode a prefixed global id.
    pub fn encode_prefixed(&self) -> String {
        let hash = Self::build_harsh_prefixed().encode(&[
            // scope
            1,
            // version
            2,
            self.kind as u64,
            self.database_id,
        ]);

        format!("{}_{}", self.kind.as_prefix(), hash)
    }

    fn parse_values(values: &[u64]) -> Result<Self, ErrorKind> {
        let scope = values.first().cloned().ok_or(ErrorKind::MissingScope)?;

        if scope != 1 {
            return Err(ErrorKind::UnknownScope(scope));
        }

        let version = values.get(1).cloned().ok_or(ErrorKind::MissingVersion)?;
        if version != 2 {
            return Err(ErrorKind::UnknownVersion(version));
        }

        let ty_raw = values.get(2).cloned().ok_or(ErrorKind::MissingNodeType)?;
        let ty_parsed = NodeKind::from_num(ty_raw).ok_or(ErrorKind::UnknownNodeType(ty_raw))?;

        let db_id = values.get(3).cloned().ok_or(ErrorKind::MissingDatabaseId)?;

        Ok(Self {
            kind: ty_parsed,
            database_id: db_id,
        })
    }

    /// Parse a prefixed global id.
    pub fn parse_prefixed(hash: &str) -> Result<Self, GlobalIdParseError> {
        let (prefix, value) = hash
            .split_once('_')
            .ok_or_else(|| GlobalIdParseError::new(hash, ErrorKind::MissingPrefix))?;

        if prefix.is_empty() {
            return Err(GlobalIdParseError::new(hash, ErrorKind::MissingPrefix));
        }

        let ty_prefix = NodeKind::parse_prefix(prefix).ok_or_else(|| {
            GlobalIdParseError::new(hash, ErrorKind::UnknownPrefix(prefix.to_string()))
        })?;

        let values = Self::build_harsh_prefixed()
            .decode(value)
            .map_err(|err| GlobalIdParseError::new(hash, ErrorKind::Decode(err.to_string())))?;

        let s = Self::parse_values(&values).map_err(|kind| GlobalIdParseError::new(hash, kind))?;

        if ty_prefix != s.kind {
            return Err(GlobalIdParseError::new(hash, ErrorKind::PrefixTypeMismatch));
        }

        Ok(s)
    }

    /// Encode a non-prefixed global id.
    ///
    /// Note: URL ids use a different alphabet than prefixed ids.
    pub fn encode_url(&self) -> String {
        Self::build_harsh_url().encode(&[
            // scope
            1,
            // version
            2,
            self.kind as u64,
            self.database_id,
        ])
    }

    /// Parse a non-prefixed URL global id variant.
    ///
    /// Note: URL ids use a different alphabet than prefixed ids.
    pub fn parse_url(hash: &str) -> Result<Self, GlobalIdParseError> {
        let values = Self::build_harsh_url()
            .decode(hash)
            .map_err(|err| GlobalIdParseError::new(hash, ErrorKind::Decode(err.to_string())))?;

        Self::parse_values(&values).map_err(|kind| GlobalIdParseError::new(hash, kind))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalIdParseError {
    id: String,
    kind: ErrorKind,
}

impl GlobalIdParseError {
    fn new(id: impl Into<String>, kind: ErrorKind) -> Self {
        Self {
            id: id.into(),
            kind,
        }
    }
}

/// Error type for parsing of [`GlobalId`]s.
// Note: kept private on purpose, not useful to export.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
enum ErrorKind {
    MissingPrefix,
    UnknownPrefix(String),
    PrefixTypeMismatch,
    MissingScope,
    UnknownScope(u64),
    MissingVersion,
    UnknownVersion(u64),
    MissingNodeType,
    UnknownNodeType(u64),
    MissingDatabaseId,
    Decode(String),
}

impl std::fmt::Display for GlobalIdParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "could not parse global id '{}': ", self.id)?;

        match &self.kind {
            ErrorKind::UnknownPrefix(p) => {
                write!(f, "unknown type prefix '{}'", p)
            }
            ErrorKind::Decode(s) => {
                write!(f, "decode error: {}", s)
            }
            ErrorKind::MissingScope => {
                write!(f, "missing scope value")
            }
            ErrorKind::UnknownScope(x) => {
                write!(f, "unknown scope value {}", x)
            }
            ErrorKind::MissingVersion => {
                write!(f, "missing version value")
            }
            ErrorKind::UnknownVersion(v) => {
                write!(f, "unknown version value {}", v)
            }
            ErrorKind::UnknownNodeType(t) => {
                write!(f, "unknown node type '{}'", t)
            }
            ErrorKind::MissingPrefix => write!(f, "missing prefix"),
            ErrorKind::PrefixTypeMismatch => write!(f, "prefix type mismatch"),
            ErrorKind::MissingNodeType => write!(f, "missing node type"),
            ErrorKind::MissingDatabaseId => write!(f, "missing database id"),
        }
    }
}

impl std::error::Error for GlobalIdParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_id() {
        // Roundtrip.
        let x1 = GlobalId {
            kind: NodeKind::DeployApp,
            database_id: 123,
        };
        assert_eq!(Ok(x1), GlobalId::parse_prefixed(&x1.encode_prefixed()),);
        assert_eq!(Ok(x1), GlobalId::parse_url(&x1.encode_url()));

        assert_eq!(
            GlobalId::parse_prefixed("da_MRrWI0t5U582"),
            Ok(GlobalId {
                kind: NodeKind::DeployApp,
                database_id: 273,
            })
        );

        // Error conditions.
        assert_eq!(
            GlobalId::parse_prefixed("oOtQIDI7q").err().unwrap().kind,
            ErrorKind::MissingPrefix,
        );
        assert_eq!(
            GlobalId::parse_prefixed("oOtQIDI7q").err().unwrap().kind,
            ErrorKind::MissingPrefix,
        );
        assert_eq!(
            GlobalId::parse_prefixed("_oOtQIDI7q").err().unwrap().kind,
            ErrorKind::MissingPrefix,
        );
        assert_eq!(
            GlobalId::parse_prefixed("lala_oOtQIDI7q")
                .err()
                .unwrap()
                .kind,
            ErrorKind::UnknownPrefix("lala".to_string()),
        );

        let kind = GlobalId::parse_prefixed("da_xxx").err().unwrap().kind;
        assert!(matches!(kind, ErrorKind::Decode(_)));
    }

    #[test]
    fn test_global_id_parse_values() {
        assert_eq!(GlobalId::parse_values(&[]), Err(ErrorKind::MissingScope),);
        assert_eq!(
            GlobalId::parse_values(&[2]),
            Err(ErrorKind::UnknownScope(2)),
        );
        assert_eq!(GlobalId::parse_values(&[1]), Err(ErrorKind::MissingVersion),);
        assert_eq!(
            GlobalId::parse_values(&[1, 999]),
            Err(ErrorKind::UnknownVersion(999)),
        );
        assert_eq!(
            GlobalId::parse_values(&[1, 2]),
            Err(ErrorKind::MissingNodeType),
        );
        assert_eq!(
            GlobalId::parse_values(&[1, 2, 99999]),
            Err(ErrorKind::UnknownNodeType(99999)),
        );
        assert_eq!(
            GlobalId::parse_values(&[1, 2, 1]),
            Err(ErrorKind::MissingDatabaseId),
        );
        assert_eq!(
            GlobalId::parse_values(&[1, 2, 1, 1]),
            Ok(GlobalId {
                kind: NodeKind::SocialAuth,
                database_id: 1,
            }),
        );
    }
}
