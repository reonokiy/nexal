use nexal_features::Feature;
use nexal_features::Features;
use nexal_protocol::models::ImageDetail;
use nexal_protocol::openai_models::ModelInfo;

pub(crate) fn can_request_original_image_detail(
    features: &Features,
    model_info: &ModelInfo,
) -> bool {
    model_info.supports_image_detail_original && features.enabled(Feature::ImageDetailOriginal)
}

pub(crate) fn normalize_output_image_detail(
    features: &Features,
    model_info: &ModelInfo,
    detail: Option<ImageDetail>,
) -> Option<ImageDetail> {
    match detail {
        Some(ImageDetail::Original) if can_request_original_image_detail(features, model_info) => {
            Some(ImageDetail::Original)
        }
        Some(ImageDetail::Original) | Some(_) | None => None,
    }
}


