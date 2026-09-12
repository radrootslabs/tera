use super::{SubmissionCaptureError as E, media::Prepared};
use crate::runtime::product_surface::{
    AddCommandType, ComposerFormInput, CreateAsk, CreateFoodAvailability, CreatePhotoUpdate,
    CreateUpdate, Phase1AddCommand,
};
use radroots_event::food::availability::{
    FoodAvailabilityDetails, FoodAvailabilityDetailsParts, FoodAvailabilityImage,
    FoodAvailabilityStatus, FoodContent, FoodCurrency, FoodIdentifier, FoodImageDimensions,
    FoodPrice, FoodPublishedAt, FoodQuantity, FoodText, FoodUnit,
};

mod calendar;

pub(super) fn command(
    input: &ComposerFormInput,
    media: &[Prepared],
    time: u64,
) -> Result<Phase1AddCommand, E> {
    Ok(match input.command_type {
        AddCommandType::CreateUpdate => {
            if !media.is_empty() {
                return Err(E::InvalidInput("media_not_allowed"));
            }
            Phase1AddCommand::CreateUpdate(
                CreateUpdate::new(input.content.clone())
                    .map_err(|_| E::InvalidInput("invalid_update"))?,
            )
        }
        AddCommandType::CreatePhotoUpdate => Phase1AddCommand::CreatePhotoUpdate(
            CreatePhotoUpdate::new(content(&input.content, media)?, post_images(media)?)
                .map_err(|_| E::InvalidInput("invalid_photo_update"))?,
        ),
        AddCommandType::CreateAsk => Phase1AddCommand::CreateAsk(
            CreateAsk::new(content(&input.content, media)?, post_images(media)?)
                .map_err(|_| E::InvalidInput("invalid_ask"))?,
        ),
        AddCommandType::CreateEvent => {
            Phase1AddCommand::CreateEvent(calendar::command(input, media)?)
        }
        AddCommandType::CreateFoodAvailability => {
            Phase1AddCommand::CreateFoodAvailability(food(input, media, time)?)
        }
    })
}

fn post_images(media: &[Prepared]) -> Result<Vec<radroots_event::post::AuthoredPostImage>, E> {
    media.iter().map(Prepared::post_image).collect()
}

fn content(input: &str, media: &[Prepared]) -> Result<String, E> {
    if input.trim().is_empty() {
        return Err(E::InvalidInput("content_required"));
    }
    let mut content = input.to_owned();
    for item in media {
        let url = item.descriptor.url().as_str();
        match content.match_indices(url).count() {
            0 => {
                if !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str(url);
            }
            1 => {}
            _ => return Err(E::InvalidInput("duplicate_media_reference")),
        }
    }
    Ok(content)
}

fn required<'a>(value: Option<&'a str>, code: &'static str) -> Result<&'a str, E> {
    value.ok_or(E::InvalidInput(code))
}

fn food(
    input: &ComposerFormInput,
    media: &[Prepared],
    time: u64,
) -> Result<CreateFoodAvailability, E> {
    let unit = FoodUnit::parse(required(input.unit.as_deref(), "food_unit_required")?)
        .map_err(|_| E::InvalidInput("invalid_food_unit"))?;
    let images = media
        .iter()
        .map(|item| {
            Ok(FoodAvailabilityImage::new(
                item.image()?,
                FoodImageDimensions::new(item.input.width, item.input.height)
                    .map_err(|_| E::InvalidInput("invalid_image_dimensions"))?,
            ))
        })
        .collect::<Result<Vec<_>, E>>()?;
    let details = FoodAvailabilityDetails::new(FoodAvailabilityDetailsParts {
        content: FoodContent::new(input.content.clone())
            .map_err(|_| E::InvalidInput("invalid_food_content"))?,
        identifier: FoodIdentifier::parse(required(
            input.identifier.as_deref(),
            "food_identifier_required",
        )?)
        .map_err(|_| E::InvalidInput("invalid_food_identifier"))?,
        title: FoodText::new(required(input.title.as_deref(), "food_title_required")?)
            .map_err(|_| E::InvalidInput("invalid_food_title"))?,
        summary: FoodText::new(required(input.summary.as_deref(), "food_summary_required")?)
            .map_err(|_| E::InvalidInput("invalid_food_summary"))?,
        published_at: FoodPublishedAt::new(input.food_published_at_unix_s.unwrap_or(time))
            .map_err(|_| E::InvalidInput("invalid_food_published_at"))?,
        location: FoodText::new(required(
            input.location.as_deref(),
            "food_location_required",
        )?)
        .map_err(|_| E::InvalidInput("invalid_food_location"))?,
        price: FoodPrice::new(
            required(input.price_amount.as_deref(), "food_price_required")?,
            FoodCurrency::parse(required(
                input.currency.as_deref(),
                "food_currency_required",
            )?)
            .map_err(|_| E::InvalidInput("invalid_food_currency"))?,
            unit,
        )
        .map_err(|_| E::InvalidInput("invalid_food_price"))?,
        quantity: input
            .quantity
            .as_deref()
            .map(|value| FoodQuantity::new(value, unit))
            .transpose()
            .map_err(|_| E::InvalidInput("invalid_food_quantity"))?,
        status: FoodAvailabilityStatus::parse(input.food_status.as_deref().unwrap_or("active"))
            .map_err(|_| E::InvalidInput("invalid_food_status"))?,
        images,
    })
    .map_err(|_| E::InvalidInput("invalid_food_availability"))?;
    Ok(CreateFoodAvailability::new(details))
}
