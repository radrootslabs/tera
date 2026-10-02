"""Owned privacy and contact declarations for the implemented public product."""

from __future__ import annotations

from typing import Any

SUPPORT_EMAIL = "support@radroots.org"
CAMERA_PURPOSE = (
    "Tera uses the camera only when you choose to add a photo to a public post."
)


def manifest() -> dict[str, Any]:
    """Publicly posted identity/profile/content/media remain linked off device."""
    data_types = [
        "NSPrivacyCollectedDataTypeName",
        "NSPrivacyCollectedDataTypeUserID",
        "NSPrivacyCollectedDataTypePhysicalAddress",
        "NSPrivacyCollectedDataTypePhotosorVideos",
        "NSPrivacyCollectedDataTypeOtherUserContent",
    ]
    return {
        "NSPrivacyTracking": False,
        "NSPrivacyTrackingDomains": [],
        "NSPrivacyCollectedDataTypes": [
            {
                "NSPrivacyCollectedDataType": value,
                "NSPrivacyCollectedDataTypeLinked": True,
                "NSPrivacyCollectedDataTypeTracking": False,
                "NSPrivacyCollectedDataTypePurposes": [
                    "NSPrivacyCollectedDataTypePurposeAppFunctionality"
                ],
            }
            for value in data_types
        ],
        "NSPrivacyAccessedAPITypes": [
            {
                "NSPrivacyAccessedAPIType": "NSPrivacyAccessedAPICategoryFileTimestamp",
                "NSPrivacyAccessedAPITypeReasons": ["C617.1"],
            },
            {
                "NSPrivacyAccessedAPIType": "NSPrivacyAccessedAPICategoryDiskSpace",
                "NSPrivacyAccessedAPITypeReasons": ["E174.1"],
            },
            {
                "NSPrivacyAccessedAPIType": "NSPrivacyAccessedAPICategoryUserDefaults",
                "NSPrivacyAccessedAPITypeReasons": ["CA92.1"],
            },
        ],
    }


def validate_manifest(document: dict[str, Any]) -> None:
    # JSON preserves boolean versus numeric types; Python equality aliases 0
    # and False and cannot independently validate a privacy declaration.
    import json

    try:
        actual = json.dumps(document, sort_keys=True, allow_nan=False)
    except (TypeError, ValueError) as error:
        raise ValueError("privacy manifest contains unsupported values") from error
    if actual != json.dumps(manifest(), sort_keys=True):
        raise ValueError("privacy manifest differs from implemented public product")


def validate_purposes(document: dict[str, Any]) -> None:
    if document.get("NSCameraUsageDescription") != CAMERA_PURPOSE:
        raise ValueError("camera purpose differs from implemented public posting")
    if document.get("TeraSupportEmail") != SUPPORT_EMAIL:
        raise ValueError("support contact differs from approved canonical contact")
