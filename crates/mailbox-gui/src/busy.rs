//! In-flight UI activity — spinners on toolbar / settings while work runs.

use mailbox_core::types::SpecialFolderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BusyFlags {
    pub sync: bool,
    pub archive: bool,
    pub trash: bool,
    pub snooze: bool,
    pub mail_test: bool,
    pub oauth: bool,
    pub ailiak: bool,
    pub send: bool,
}

impl BusyFlags {
    pub fn any(self) -> bool {
        self.sync
            || self.archive
            || self.trash
            || self.snooze
            || self.mail_test
            || self.oauth
            || self.ailiak
            || self.send
    }

    pub fn moving(self) -> bool {
        self.archive || self.trash
    }

    pub fn set_move(&mut self, kind: SpecialFolderKind) {
        match kind {
            SpecialFolderKind::Archive => self.archive = true,
            SpecialFolderKind::Trash => self.trash = true,
        }
    }

    pub fn clear_move(&mut self) {
        self.archive = false;
        self.trash = false;
    }

    pub fn clear_network(&mut self) {
        self.sync = false;
        self.clear_move();
        self.mail_test = false;
        self.oauth = false;
        self.ailiak = false;
        self.send = false;
    }
}
