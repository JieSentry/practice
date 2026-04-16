use std::{cell::RefCell, fmt::Debug, rc::Rc, sync::Arc};

use crate::{
    DetectionTemplate, Localization,
    detect::{
        CASH_SHOP_TEMPLATE, CHANGE_CHANNEL_TEMPLATE, FAMILIAR_LEVEL_BUTTON_TEMPLATE,
        FAMILIAR_SAVE_BUTTON_TEMPLATE, HEXA_BOOSTER_BUTTON_TEMPLATE, HEXA_CONVERT_BUTTON_TEMPLATE,
        HEXA_ERDA_CONVERSION_BUTTON_TEMPLATE, HEXA_MAX_BUTTON_TEMPLATE,
        LIE_DETECTOR_SHAPE_PREPARE_TEMPLATE,
        LIE_DETECTOR_TRANSPARENT_SHAPE_TEMPLATE, LIE_DETECTOR_VIOLETTA_TEMPLATE,
        POPUP_CANCEL_NEW_TEMPLATE, POPUP_CANCEL_OLD_TEMPLATE, POPUP_CONFIRM_TEMPLATE,
        POPUP_END_CHAT_TEMPLATE, POPUP_NEXT_TEMPLATE, POPUP_OK_NEW_TEMPLATE, POPUP_OK_OLD_TEMPLATE,
        POPUP_YES_TEMPLATE, TIMER_TEMPLATE, to_base64_from_mat,TOF_FATE_CHARACTER_TEMPLATE,
        TOF_ASK_BUTTON_TEMPLATE,TOF_YES_TEMPLATE, TOF_NEXT_TEMPLATE,
    },
    ecs::Resources,
    utils::{self, DatasetDir},
};

/// A service for handling localization-related incoming requests.
pub trait LocalizationService: Debug {
    /// Retrieves the default base64-encoded PNG for template `template`.
    fn template(&self, template: DetectionTemplate) -> String;

    /// Updates the currently in use [`Localization`] with new `localization`.
    fn update_localization(&mut self, localization: Localization);

    /// Saves the currently captured image to the `datasets` folder.
    fn save_capture_image(&self, resources: &mut Resources, is_grayscale: bool);
}

#[derive(Debug)]
pub struct DefaultLocalizationService {
    localization: Rc<RefCell<Arc<Localization>>>,
}

impl DefaultLocalizationService {
    pub fn new(localization: Rc<RefCell<Arc<Localization>>>) -> Self {
        Self { localization }
    }
}

impl LocalizationService for DefaultLocalizationService {
    fn template(&self, template: DetectionTemplate) -> String {
        let template = match template {
            DetectionTemplate::CashShop => &CASH_SHOP_TEMPLATE,
            DetectionTemplate::ChangeChannel => &CHANGE_CHANNEL_TEMPLATE,
            DetectionTemplate::Timer => &TIMER_TEMPLATE,
            DetectionTemplate::PopupConfirm => &POPUP_CONFIRM_TEMPLATE,
            DetectionTemplate::PopupYes => &POPUP_YES_TEMPLATE,
            DetectionTemplate::PopupNext => &POPUP_NEXT_TEMPLATE,
            DetectionTemplate::PopupEndChat => &POPUP_END_CHAT_TEMPLATE,
            DetectionTemplate::PopupOkNew => &POPUP_OK_NEW_TEMPLATE,
            DetectionTemplate::PopupOkOld => &POPUP_OK_OLD_TEMPLATE,
            DetectionTemplate::PopupCancelNew => &POPUP_CANCEL_NEW_TEMPLATE,
            DetectionTemplate::PopupCancelOld => &POPUP_CANCEL_OLD_TEMPLATE,
            DetectionTemplate::FamiliarsLevelSort => &FAMILIAR_LEVEL_BUTTON_TEMPLATE,
            DetectionTemplate::FamiliarsSaveButton => &FAMILIAR_SAVE_BUTTON_TEMPLATE,
            DetectionTemplate::HexaErdaConversionButton => &HEXA_ERDA_CONVERSION_BUTTON_TEMPLATE,
            DetectionTemplate::HexaBoosterButton => &HEXA_BOOSTER_BUTTON_TEMPLATE,
            DetectionTemplate::HexaMaxButton => &HEXA_MAX_BUTTON_TEMPLATE,
            DetectionTemplate::HexaConvertButton => &HEXA_CONVERT_BUTTON_TEMPLATE,
            DetectionTemplate::LieDetectorNew => &LIE_DETECTOR_TRANSPARENT_SHAPE_TEMPLATE,
            DetectionTemplate::LieDetectorOld => &LIE_DETECTOR_VIOLETTA_TEMPLATE,
            DetectionTemplate::LieDetectorShapePrepare => &LIE_DETECTOR_SHAPE_PREPARE_TEMPLATE,
            DetectionTemplate::TofFateCharacter => &TOF_FATE_CHARACTER_TEMPLATE,  
            DetectionTemplate::TofAskButton => &TOF_ASK_BUTTON_TEMPLATE,
            DetectionTemplate::TofYes => &TOF_YES_TEMPLATE,  
            DetectionTemplate::TofNext => &TOF_NEXT_TEMPLATE,
        };

        to_base64_from_mat(template).expect("convert successfully")
    }

    fn update_localization(&mut self, localization: Localization) {
        *self.localization.borrow_mut() = Arc::new(localization);
    }

    fn save_capture_image(&self, resources: &mut Resources, is_grayscale: bool) {
        if let Some(detector) = resources.detector.as_ref() {
            if is_grayscale {
                utils::save_image_to_default(detector.grayscale(), DatasetDir::Root);
            } else {
                utils::save_image_to_default(&detector.mat(), DatasetDir::Root);
            }
        }
    }
}
