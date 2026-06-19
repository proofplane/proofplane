use std::str::FromStr;

use super::DomainError;

/// A data-plane capability a token may hold within its workspace. Mirrors the
/// read/write split across the three data-plane resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkspacePermission {
    ReadEvidenceRequests,
    WriteEvidenceRequests,
    ReadEvidenceSubmissions,
    WriteEvidenceSubmissions,
    ReadControls,
    WriteControls,
}

impl WorkspacePermission {
    pub const ALL: [WorkspacePermission; 6] = [
        Self::ReadEvidenceRequests,
        Self::WriteEvidenceRequests,
        Self::ReadEvidenceSubmissions,
        Self::WriteEvidenceSubmissions,
        Self::ReadControls,
        Self::WriteControls,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadEvidenceRequests => "read_evidence_requests",
            Self::WriteEvidenceRequests => "write_evidence_requests",
            Self::ReadEvidenceSubmissions => "read_evidence_submissions",
            Self::WriteEvidenceSubmissions => "write_evidence_submissions",
            Self::ReadControls => "read_controls",
            Self::WriteControls => "write_controls",
        }
    }

    fn bit(self) -> u8 {
        match self {
            Self::ReadEvidenceRequests => 1 << 0,
            Self::WriteEvidenceRequests => 1 << 1,
            Self::ReadEvidenceSubmissions => 1 << 2,
            Self::WriteEvidenceSubmissions => 1 << 3,
            Self::ReadControls => 1 << 4,
            Self::WriteControls => 1 << 5,
        }
    }
}

impl FromStr for WorkspacePermission {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "read_evidence_requests" => Ok(Self::ReadEvidenceRequests),
            "write_evidence_requests" => Ok(Self::WriteEvidenceRequests),
            "read_evidence_submissions" => Ok(Self::ReadEvidenceSubmissions),
            "write_evidence_submissions" => Ok(Self::WriteEvidenceSubmissions),
            "read_controls" => Ok(Self::ReadControls),
            "write_controls" => Ok(Self::WriteControls),
            _ => Err(DomainError::InvalidEnumValue {
                field: "permission",
                value: value.to_owned(),
            }),
        }
    }
}

/// A `Copy` set of granted workspace permissions, small enough to live inside
/// `ApiTokenContext` and be checked in-memory on every data-plane request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkspacePermissions {
    bits: u8,
}

impl WorkspacePermissions {
    pub fn none() -> Self {
        Self { bits: 0 }
    }

    pub fn all() -> Self {
        Self::from_iter(WorkspacePermission::ALL)
    }

    pub fn insert(&mut self, permission: WorkspacePermission) {
        self.bits |= permission.bit();
    }

    pub fn has(self, permission: WorkspacePermission) -> bool {
        self.bits & permission.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub fn iter(self) -> impl Iterator<Item = WorkspacePermission> {
        WorkspacePermission::ALL
            .into_iter()
            .filter(move |permission| self.has(*permission))
    }
}

impl FromIterator<WorkspacePermission> for WorkspacePermissions {
    fn from_iter<T: IntoIterator<Item = WorkspacePermission>>(iter: T) -> Self {
        let mut permissions = Self::none();
        for permission in iter {
            permissions.insert(permission);
        }
        permissions
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{WorkspacePermission, WorkspacePermissions};

    #[test]
    fn permission_round_trips_through_string() {
        for permission in WorkspacePermission::ALL {
            assert_eq!(
                WorkspacePermission::from_str(permission.as_str()).unwrap(),
                permission
            );
        }
    }

    #[test]
    fn unknown_permission_string_is_rejected() {
        assert!(WorkspacePermission::from_str("delete_everything").is_err());
    }

    #[test]
    fn set_tracks_only_granted_permissions() {
        let permissions = WorkspacePermissions::from_iter([
            WorkspacePermission::ReadEvidenceRequests,
            WorkspacePermission::ReadControls,
        ]);

        assert!(permissions.has(WorkspacePermission::ReadEvidenceRequests));
        assert!(permissions.has(WorkspacePermission::ReadControls));
        assert!(!permissions.has(WorkspacePermission::WriteEvidenceRequests));
        assert!(!permissions.is_empty());
    }

    #[test]
    fn all_grants_every_permission_and_none_grants_nothing() {
        let all = WorkspacePermissions::all();
        for permission in WorkspacePermission::ALL {
            assert!(all.has(permission));
        }
        assert!(WorkspacePermissions::none().is_empty());
    }
}
