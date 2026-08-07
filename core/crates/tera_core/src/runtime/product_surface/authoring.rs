use radroots_event::{
    calendar::{AuthoredCalendarDateEvent, AuthoredCalendarTimeEvent},
    food::availability::FoodAvailabilityDetails,
    post::{
        AuthoredAsk, AuthoredPhotoUpdate, AuthoredPostError, AuthoredPostImage, AuthoredUpdate,
        deletion::AuthoredNip09DeletionRequest,
    },
};
use radroots_event_codec::authoring::{AuthoredEventPlan, AuthoredPlanError};

use super::AddCommandType;

/// A strict `CreateUpdate` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateUpdate(AuthoredUpdate);

impl CreateUpdate {
    pub fn new(content: impl Into<String>) -> Result<Self, AuthoredPostError> {
        AuthoredUpdate::new(content).map(Self)
    }

    pub const fn authored(&self) -> &AuthoredUpdate {
        &self.0
    }
}

/// A strict `CreatePhotoUpdate` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatePhotoUpdate(AuthoredPhotoUpdate);

impl CreatePhotoUpdate {
    pub fn new(
        content: impl Into<String>,
        images: Vec<AuthoredPostImage>,
    ) -> Result<Self, AuthoredPostError> {
        AuthoredPhotoUpdate::new(content, images).map(Self)
    }

    pub const fn authored(&self) -> &AuthoredPhotoUpdate {
        &self.0
    }
}

/// A strict `CreateAsk` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateAsk(AuthoredAsk);

impl CreateAsk {
    pub fn new(
        question: impl Into<String>,
        images: Vec<AuthoredPostImage>,
    ) -> Result<Self, AuthoredPostError> {
        AuthoredAsk::new(question, images).map(Self)
    }

    pub const fn authored(&self) -> &AuthoredAsk {
        &self.0
    }
}

/// A strict `CreateEvent` command with an explicit all-day or timed profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateEvent(Box<CreateEventProfile>);

#[derive(Clone, Debug, PartialEq, Eq)]
enum CreateEventProfile {
    Date(AuthoredCalendarDateEvent),
    Time(AuthoredCalendarTimeEvent),
}

impl CreateEvent {
    pub fn date(event: AuthoredCalendarDateEvent) -> Self {
        Self(Box::new(CreateEventProfile::Date(event)))
    }

    pub fn time(event: AuthoredCalendarTimeEvent) -> Self {
        Self(Box::new(CreateEventProfile::Time(event)))
    }
}

/// A strict `CreateFoodAvailability` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateFoodAvailability(FoodAvailabilityDetails);

impl CreateFoodAvailability {
    pub const fn new(details: FoodAvailabilityDetails) -> Self {
        Self(details)
    }

    pub const fn authored(&self) -> &FoodAvailabilityDetails {
        &self.0
    }
}

/// The only five Phase 1 Add commands accepted by focused mobile authoring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase1AddCommand {
    CreateUpdate(CreateUpdate),
    CreatePhotoUpdate(CreatePhotoUpdate),
    CreateAsk(CreateAsk),
    CreateEvent(CreateEvent),
    CreateFoodAvailability(CreateFoodAvailability),
}

/// Standard revision behavior for a Phase 1 authored profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase1ReplacementPolicy {
    /// Regular kind-1 events have no edit convention; retract and create are
    /// independent authored operations whose partial effects remain visible.
    RetractThenCreate,
    /// Addressable events replace the current head by reusing their stable `d`.
    AddressableReplacement,
}

impl Phase1AddCommand {
    pub const fn command_type(&self) -> AddCommandType {
        match self {
            Self::CreateUpdate(_) => AddCommandType::CreateUpdate,
            Self::CreatePhotoUpdate(_) => AddCommandType::CreatePhotoUpdate,
            Self::CreateAsk(_) => AddCommandType::CreateAsk,
            Self::CreateEvent(_) => AddCommandType::CreateEvent,
            Self::CreateFoodAvailability(_) => AddCommandType::CreateFoodAvailability,
        }
    }

    pub const fn replacement_policy(&self) -> Phase1ReplacementPolicy {
        match self {
            Self::CreateUpdate(_) | Self::CreatePhotoUpdate(_) | Self::CreateAsk(_) => {
                Phase1ReplacementPolicy::RetractThenCreate
            }
            Self::CreateEvent(_) | Self::CreateFoodAvailability(_) => {
                Phase1ReplacementPolicy::AddressableReplacement
            }
        }
    }

    /// Binds the validated command to one exact timestamp and expected author.
    pub fn authored_plan(
        &self,
        created_at: u64,
        expected_author: impl AsRef<str>,
    ) -> Result<AuthoredEventPlan, AuthoredPlanError> {
        let expected_author = expected_author.as_ref();
        match self {
            Self::CreateUpdate(command) => {
                AuthoredEventPlan::from_update(command.authored(), created_at, expected_author)
            }
            Self::CreatePhotoUpdate(command) => AuthoredEventPlan::from_photo_update(
                command.authored(),
                created_at,
                expected_author,
            ),
            Self::CreateAsk(command) => {
                AuthoredEventPlan::from_ask(command.authored(), created_at, expected_author)
            }
            Self::CreateEvent(command) => match command.0.as_ref() {
                CreateEventProfile::Date(event) => {
                    AuthoredEventPlan::from_calendar_date_event(event, created_at, expected_author)
                }
                CreateEventProfile::Time(event) => {
                    AuthoredEventPlan::from_calendar_time_event(event, created_at, expected_author)
                }
            },
            Self::CreateFoodAvailability(command) => AuthoredEventPlan::from_food_availability(
                command.authored(),
                created_at,
                expected_author,
            ),
        }
    }
}

/// Builds the independent strict NIP-09 plan used for retraction or withdrawal.
///
/// This function intentionally does not combine retraction and replacement
/// into one purportedly atomic operation.
pub fn phase1_retraction_plan(
    request: &AuthoredNip09DeletionRequest,
    created_at: u64,
    expected_author: impl AsRef<str>,
) -> Result<AuthoredEventPlan, AuthoredPlanError> {
    AuthoredEventPlan::from_nip09_deletion_request(request, created_at, expected_author)
}

#[cfg(test)]
mod tests {
    use super::*;
    use radroots_blossom::{BlobDescriptor, BlobUrl, MediaType, Sha256};
    use radroots_event::{
        calendar::CalendarDate,
        envelope::kind::{
            KIND_CALENDAR_DATE_EVENT, KIND_CALENDAR_TIME_EVENT, KIND_CLASSIFIED_LISTING, KIND_POST,
        },
        food::availability::{
            FoodAvailabilityDetailsParts, FoodAvailabilityStatus, FoodContent, FoodCurrency,
            FoodIdentifier, FoodPrice, FoodPublishedAt, FoodText, FoodUnit,
        },
        media::AuthoredImage,
        post::PostImageDimensions,
        post::deletion::{AuthoredNip09DeletionRequest, Nip09DeletionEventTarget},
    };

    const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn focused_commands_bind_only_locked_wire_profiles() {
        let date = AuthoredCalendarDateEvent::new(
            "market-day",
            "Saturday Market",
            CalendarDate::parse("2026-08-08").unwrap(),
        )
        .unwrap();
        let time = AuthoredCalendarTimeEvent::new("farm-tour", "Farm Tour", 1_784_380_800).unwrap();
        let image = post_image();
        let commands = [
            Phase1AddCommand::CreateUpdate(CreateUpdate::new("Harvest update").unwrap()),
            Phase1AddCommand::CreatePhotoUpdate(
                CreatePhotoUpdate::new(format!("Harvest photo {}", image.url()), vec![image])
                    .unwrap(),
            ),
            Phase1AddCommand::CreateAsk(CreateAsk::new("Who has basil?", Vec::new()).unwrap()),
            Phase1AddCommand::CreateEvent(CreateEvent::date(date)),
            Phase1AddCommand::CreateEvent(CreateEvent::time(time)),
            Phase1AddCommand::CreateFoodAvailability(CreateFoodAvailability::new(food())),
        ];
        let expected = [
            (AddCommandType::CreateUpdate, KIND_POST),
            (AddCommandType::CreatePhotoUpdate, KIND_POST),
            (AddCommandType::CreateAsk, KIND_POST),
            (AddCommandType::CreateEvent, KIND_CALENDAR_DATE_EVENT),
            (AddCommandType::CreateEvent, KIND_CALENDAR_TIME_EVENT),
            (
                AddCommandType::CreateFoodAvailability,
                KIND_CLASSIFIED_LISTING,
            ),
        ];
        for (command, (command_type, kind)) in commands.iter().zip(expected) {
            let plan = command.authored_plan(1_784_347_200, AUTHOR).unwrap();
            assert_eq!(command.command_type(), command_type);
            assert_eq!(plan.body().kind(), kind);
            assert_ne!(plan.body().kind(), 20);
        }
    }

    #[test]
    fn revision_and_retraction_semantics_are_explicit() {
        let update = Phase1AddCommand::CreateUpdate(CreateUpdate::new("new post").unwrap());
        assert_eq!(
            update.replacement_policy(),
            Phase1ReplacementPolicy::RetractThenCreate
        );
        let event = Phase1AddCommand::CreateEvent(CreateEvent::time(
            AuthoredCalendarTimeEvent::new("farm-tour", "Farm Tour", 1_784_380_800).unwrap(),
        ));
        assert_eq!(
            event.replacement_policy(),
            Phase1ReplacementPolicy::AddressableReplacement
        );

        let request = AuthoredNip09DeletionRequest::new(
            "retracted",
            vec![Nip09DeletionEventTarget::parse("b".repeat(64), KIND_POST).unwrap()],
            Vec::new(),
        )
        .unwrap();
        let plan = phase1_retraction_plan(&request, 1_784_347_201, AUTHOR).unwrap();
        assert_eq!(plan.body().kind(), 5);
    }

    fn food() -> FoodAvailabilityDetails {
        FoodAvailabilityDetails::new(FoodAvailabilityDetailsParts {
            content: FoodContent::new("Carrots available this week.").unwrap(),
            identifier: FoodIdentifier::parse("nantes-carrots").unwrap(),
            title: FoodText::new("Nantes Carrots").unwrap(),
            summary: FoodText::new("Fresh bunches").unwrap(),
            published_at: FoodPublishedAt::new(1_784_347_100).unwrap(),
            location: FoodText::new("Central Saanich, BC").unwrap(),
            price: FoodPrice::new("3", FoodCurrency::parse("CAD").unwrap(), FoodUnit::Pound)
                .unwrap(),
            quantity: None,
            status: FoodAvailabilityStatus::Active,
            images: Vec::new(),
        })
        .unwrap()
    }

    fn post_image() -> AuthoredPostImage {
        let bytes = b"harvest-photo";
        let hash = Sha256::digest(bytes);
        let media_type = MediaType::parse("image/webp").unwrap();
        let descriptor = BlobDescriptor::new(
            BlobUrl::parse(&format!("https://media.example/{hash}.webp")).unwrap(),
            hash,
            bytes.len() as u64,
            media_type.clone(),
            1_784_347_100,
        )
        .unwrap()
        .approve_reference()
        .unwrap()
        .verify_bytes(bytes, &media_type)
        .unwrap();
        AuthoredPostImage::new(
            AuthoredImage::try_from(descriptor).unwrap(),
            PostImageDimensions::new(1200, 900).unwrap(),
            "Harvest",
        )
        .unwrap()
    }
}
