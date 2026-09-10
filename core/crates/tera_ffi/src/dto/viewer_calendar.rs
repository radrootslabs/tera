use tera_core::runtime::product_surface::ViewerCalendarContext;

use super::FfiCivilDate;

pub const TODAY_PAGE_FFI_SCHEMA_VERSION: u16 = 2;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiViewerCalendarContext {
    pub schema_version: u16,
    pub as_of_unix_s: u64,
    pub time_zone: String,
    pub civil_date: FfiCivilDate,
}

impl From<ViewerCalendarContext> for FfiViewerCalendarContext {
    fn from(value: ViewerCalendarContext) -> Self {
        Self {
            schema_version: value.version(),
            as_of_unix_s: value.as_of(),
            time_zone: value.time_zone().to_owned(),
            civil_date: value.civil_date().into(),
        }
    }
}
