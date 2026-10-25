//! What sort of repository a series targets, which selects the checkers
//! (designs/PHASE2.md §5.2).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A BitBake layer, or BitBake or the Yocto documentation: patchtest,
    /// oelint-adv, shellcheck, the documentation checks and the
    /// leftover-file check.
    #[default]
    Layer,
    /// A Django web application such as layerindex-web: patchtest's mail
    /// checks and shellcheck.
    DjangoApp,
}

impl Kind {
    /// Whether the layer checks (oelint-adv, the documentation checks, the
    /// leftover-file check and patchtest's patch suite) apply.
    pub fn is_layer(self) -> bool {
        self == Kind::Layer
    }
}
