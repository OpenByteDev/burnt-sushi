use std::sync::Arc;

use arc_swap::ArcSwap;
use enum_map::EnumMap;
use regex::RegexSet;

pub type FilterHook = shared::rpc::blocker_service::FilterHook;

#[derive(Clone, Debug)]
pub struct Filters {
    rulesets: Arc<EnumMap<FilterHook, ArcSwap<FilterRuleset>>>,
}

impl Filters {
    pub fn empty() -> Self {
        Self {
            rulesets: Arc::new(EnumMap::default()),
        }
    }

    pub fn replace_ruleset(&self, hook: FilterHook, ruleset: FilterRuleset) {
        self.rulesets[hook].store(Arc::new(ruleset));
    }

    pub fn check(&self, hook: FilterHook, request: &str) -> bool {
        let ruleset = self.rulesets[hook].load();
        ruleset.check(request)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FilterRuleset {
    pub whitelist: RegexSet,
    pub blacklist: RegexSet,
}

impl FilterRuleset {
    fn check(&self, request: &str) -> bool {
        (self.whitelist.is_empty() || self.whitelist.is_match(request))
            && !self.blacklist.is_match(request)
    }
}
