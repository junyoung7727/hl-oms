//! What the operator is shown of the margin rules (C-MARGIN).
//!
//! The view is assembled here, beside the state it reads, so that a reader
//! of the state (the node's `status`) does not depend on how the margin
//! state is laid out. Its serialised form is the `core.margin` object of
//! `status`.

use crate::core::Core;
use crate::exposure::Exposure;
use crate::ids::{InstrumentId, Ts};
use crate::input::MarginPosition;
use crate::model::{DayBase, MarginRejection, Reducing};
use crate::num::Amount;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
pub struct MarginView<'a> {
    /// Of the newest recorded report; `None` before the first one.
    pub account_value: Option<Amount>,
    pub queried_at: Option<Ts>,
    /// `now` minus the query time of the newest report.
    pub age_us: Option<u64>,
    pub fresh: bool,
    /// What the leverage gate judges with, at the last reference prices
    /// whatever their age. `None` when it has no value.
    pub account_exposure: Option<Exposure>,
    pub session: Option<u64>,
    pub seq: Option<u64>,
    pub positions: Option<&'a [MarginPosition]>,
    pub last_rejection: &'a Option<MarginRejection>,
    /// NORMAL is `None`. Otherwise the rule that tripped first, what it
    /// saw and since when.
    pub reducing: &'a Option<Reducing>,
    pub day_base: &'a Option<DayBase>,
    /// What the venue told in this link session, per configured
    /// instrument: `None` while unknown.
    pub venue_settings: BTreeMap<&'a InstrumentId, Option<VenueSettingView>>,
    /// `None` while the venue has not told it in this link session.
    pub account_mode: Option<AccountModeView<'a>>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct VenueSettingView {
    pub isolated: bool,
    pub leverage: u32,
    /// False when the instrument is held by its setting.
    pub as_configured: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct AccountModeView<'a> {
    pub mode: &'a str,
    pub allowed: bool,
}

impl Core {
    /// The margin rules as the operator sees them at `now`, or `None`
    /// without a margin policy.
    pub fn margin_view(&self, now: Ts) -> Option<MarginView<'_>> {
        self.config.policy.margin.as_ref()?;
        let margin = &self.margin;
        let report = margin.report.as_ref();
        let venue_settings = self
            .config
            .instruments
            .keys()
            .map(|instrument| {
                let seen = margin
                    .venue_settings
                    .get(instrument)
                    .map(|s| VenueSettingView {
                        isolated: s.isolated,
                        leverage: s.leverage,
                        as_configured: self.venue_setting_is_as_configured(instrument),
                    });
                (instrument, seen)
            })
            .collect();
        let account_mode = margin.account_mode.as_deref().map(|mode| AccountModeView {
            mode,
            allowed: self.account_mode_is_allowed(),
        });
        Some(MarginView {
            account_value: report.map(|r| r.account_value),
            queried_at: report.map(|r| r.queried_at),
            age_us: report.map(|r| now.saturating_sub(r.queried_at)),
            fresh: self.margin_report_is_fresh(now),
            account_exposure: self.account_exposure(),
            session: report.map(|r| r.session),
            seq: report.map(|r| r.seq),
            positions: report.map(|r| r.positions.as_slice()),
            last_rejection: &margin.last_rejection,
            reducing: &margin.reducing,
            day_base: &margin.day_base,
            venue_settings,
            account_mode,
        })
    }
}
