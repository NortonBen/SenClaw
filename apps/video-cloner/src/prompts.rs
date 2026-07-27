//! Prompt construction, ported from the original `geminiService.ts`.
//!
//! The wording is deliberately kept as-is: these prompts are tuned, and the
//! model's obedience to the "one JSON per line" and "locked voice id" rules is
//! what makes the downstream scene parser and the bulk-edit feature work.

use crate::db::CloneConfig;

/// The style dropdown's first entry means "keep the source video's own look".
pub fn is_original_style(style: &str) -> bool {
    style.contains("video gốc") || style.contains("Original Style")
}

/// Creativity knob → sampling temperature.
///
/// The slider is expressed as "how visually similar to the source", so 100%
/// similar is the most deterministic run and 0% is the most creative one.
/// "AI tự do sáng tạo" pins similarity to 0 regardless of the slider.
pub fn temperature_for(cfg: &CloneConfig) -> f64 {
    let effective = effective_similarity(cfg) as f64;
    0.1 + (1.0 - effective / 100.0) * 0.7
}

pub fn effective_similarity(cfg: &CloneConfig) -> i64 {
    if cfg.auto_magic {
        0
    } else {
        cfg.visual_similarity.clamp(0, 100)
    }
}

/// In auto-magic mode the character and background instructions are replaced by
/// open-ended creative briefs, overriding whatever the user typed.
pub fn effective_character(cfg: &CloneConfig) -> String {
    if cfg.auto_magic {
        "Hãy tự động thay đổi ngoại hình, giới tính và độ tuổi của nhân vật chính hoàn toàn mới và độc đáo nhưng vẫn cực kỳ hợp với phong cách được yêu cầu.".to_string()
    } else {
        cfg.char_description.trim().to_string()
    }
}

pub fn effective_background(cfg: &CloneConfig) -> String {
    if cfg.auto_magic {
        "Hãy tự do sáng tạo một thế giới/bối cảnh mới hoàn toàn rực rỡ và choáng ngợp dựa trên trí tưởng tượng của bạn, thoát ly khỏi bối cảnh video gốc.".to_string()
    } else {
        cfg.bg_description.trim().to_string()
    }
}

pub fn system_instruction(cfg: &CloneConfig) -> String {
    let style = cfg.style.trim();
    let char_description = effective_character(cfg);
    let bg_description = effective_background(cfg);
    let custom_dialogue = cfg.custom_dialogue.trim();
    let similarity = effective_similarity(cfg);

    let style_description = if is_original_style(style) {
        "mô tả chính xác phong cách nghệ thuật, ánh sáng và chất liệu nguyên bản trích xuất trực tiếp từ video gốc".to_string()
    } else {
        format!("áp dụng và mô tả theo phong cách: \"{style}\"")
    };

    let similarity_instruction = if similarity < 100 {
        format!(
            "\n- ĐỘ TƯƠNG ĐỒNG HÌNH ẢNH: Bạn chỉ cần giữ lại {similarity}% độ tương đồng về hình ảnh so với video gốc. Hãy sáng tạo thêm {}% chi tiết mới (ánh sáng khác, vật liệu khác, góc nhìn mới lạ) nhưng vẫn phải tuyệt đối giữ nguyên kịch bản, hành động và các thực thể (IDs).",
            100 - similarity
        )
    } else {
        "\n- ĐỘ TƯƠNG ĐỒNG HÌNH ẢNH: Hãy giữ nguyên 100% độ trung thực về hình ảnh so với video gốc.".to_string()
    };

    let char_instruction = if char_description.is_empty() {
        String::new()
    } else {
        format!("\n- THAY THẾ NHÂN VẬT CHÍNH: Thay vì mô tả nhân vật trong video gốc, hãy thay thế bằng nhân vật sau: \"{char_description}\". Giữ nguyên hành động và cảm xúc nhưng áp dụng ngoại hình mới này.")
    };

    let dialogue_instruction = if custom_dialogue.is_empty() {
        "\n- KHÔNG TỰ ĐỘNG TẠO PHỤ ĐỀ (SUBTITLES): Tuyệt đối không tự động trích xuất lời thoại từ video gốc để tạo phụ đề. Chỉ tập trung vào mô tả âm thanh môi trường và hiệu ứng.".to_string()
    } else {
        format!("\n- THAY THẾ LỜI THOẠI: Hãy bỏ qua lời thoại gốc trong video. Thay vào đó, hãy sử dụng nội dung sau để tạo lời thoại/lời dẫn (dialogue) phù hợp cho các scene: \"{custom_dialogue}\".")
    };

    let bg_instruction = if bg_description.is_empty() {
        format!("\n- TỰ ĐỘNG THAY ĐỔI BỐI CẢNH: Nếu người dùng không chỉ định bối cảnh mới, hãy tự động sáng tạo một bối cảnh mới hoàn toàn khác với bối cảnh gốc nhưng phải cực kỳ phù hợp với phong cách nghệ thuật \"{style}\". Bối cảnh mới này phải nhất quán xuyên suốt TẤT CẢ các scene.")
    } else {
        format!("\n- THAY THẾ BỐI CẢNH: Hãy thay đổi bối cảnh gốc của video thành bối cảnh sau: \"{bg_description}\". Đảm bảo bối cảnh mới này nhất quán xuyên suốt các scene và phù hợp với phong cách \"{style}\".")
    };

    format!(
        r#"Vai trò: Bạn là chuyên gia phân tích video và kỹ sư tạo prompt cho mô hình tạo video AI (Veo 3). Nhiệm vụ của bạn là xem video đầu vào, phân tách nó thành các đoạn nhỏ (mỗi đoạn đúng 8 giây) và tạo ra các file JSON mô tả kỹ thuật chính xác để tái tạo/sao chép nội dung video đó.

YÊU CẦU QUAN TRỌNG VỀ ĐỒNG NHẤT & ÂM THANH:
1. ĐỒNG NHẤT NHÂN VẬT (Character Consistency):
   - Nếu có ẢNH NHÂN VẬT MẪU, bạn PHẢI trích xuất mọi chi tiết ngoại hình (khuôn mặt, tóc, trang phục, đặc điểm nhận dạng) từ ảnh đó.
   - Mô tả nhân vật chính (CHAR_1) trong TẤT CẢ các scene phải giống hệt nhau về ngoại hình để đảm bảo tính đồng nhất tuyệt đối.
2. TẬP TRUNG VÀO ÂM THANH (Audio Focus):
   - KHÔNG tự động tạo phụ đề (subtitles) từ video gốc.
   - Tập trung mô tả chi tiết "foley_and_ambience" (âm thanh môi trường, tiếng động vật lý, nhạc nền).
   - Chỉ sử dụng phần "dialogue" nếu có yêu cầu lời thoại tùy chỉnh từ người dùng.
3. ĐỒNG NHẤT PHONG CÁCH & BỐI CẢNH MỚI:
   - Toàn bộ video phải được phân tích và {style_description}.{char_instruction}{dialogue_instruction}{bg_instruction}{similarity_instruction}
   - Trường "visual_style" và "background_lock" phải mô tả bối cảnh mới một cách nhất quán, chi tiết về: ánh sáng, vật liệu, chiều sâu và bầu không khí.
   - NẾU TỰ ĐỘNG LÀM MỚI BỐI CẢNH: Bạn phải thoát ly hoàn toàn khỏi bố cục bối cảnh cũ, chỉ giữ lại sơ đồ vị trí nhân vật để đảm bảo logic hành động. Bối cảnh mới phải là một tác phẩm nghệ thuật phù hợp hoàn hảo với phong cách "{style}".
4. NHẤT QUÁN GIỌNG NÓI TUYỆT ĐỐI (Absolute Voice Consistency):
   - Bạn PHẢI sử dụng định dạng ID giọng nói cố định: "VOICE_CHAR_1" cho nhân vật CHAR_1, "VOICE_CHAR_2" cho CHAR_2, và "NARRATOR_VOICE" cho người dẫn chuyện.
   - Tuyệt đối KHÔNG ĐƯỢC thay đổi ID giọng nói này giữa các scene. Nếu Scene 1 là "VOICE_CHAR_1" thì tất cả các scene sau cũng phải chính xác là "VOICE_CHAR_1".
   - Mọi tham chiếu trong "audio_markers" và "dialogue" đều phải sử dụng đúng ID này cho cùng một nhân vật.
5. NGÔN NGỮ ĐẦU RA (Output Language):
   - TOÀN BỘ nội dung mô tả trong các trường văn bản của JSON (bao gồm: visual_style, background_lock, voice_personality, foley_and_ambience, description, dialogue, v.v.) BẮT BUỘC phải được viết bằng TIẾNG VIỆT.
   - Sử dụng Tiếng Việt chuẩn, từ ngữ chuyên nghiệp, giàu hình ảnh và dịch thuật thật chính xác các quan sát từ video.

Quy trình xử lý:
1. Phân đoạn: Chia video thành các segment liên tiếp, mỗi segment dài đúng 8 giây.
2. Phân tích: Quan sát kỹ lưỡng từng khung hình. Chỉ mô tả những gì nhìn thấy và nghe thấy qua lăng kính phong cách yêu cầu.
3. Giữ nhất quán: Đặt ID cố định cho nhân vật (CHAR_1), bối cảnh (BACKGROUND_1) và GIỌNG NÓI (VOICE_ID).
4. Định dạng đầu ra:
   - MỖI SCENE PHẢI LÀ MỘT DÒNG JSON DUY NHẤT (VIẾT LIỀN).
   - CÁCH NHAU BỞI MỘT DÒNG TRỐNG GIỮA CÁC SCENE.
"#
    )
}

/// The per-request user turn. `last_scene_id` > 0 resumes after that segment,
/// which is how "analyse the next 8 seconds" and "redo the last one" work
/// without re-sending the scenes already accepted.
pub fn user_prompt(cfg: &CloneConfig, last_scene_id: i64, has_char_image: bool) -> String {
    let style = cfg.style.trim();
    let char_description = effective_character(cfg);
    let bg_description = effective_background(cfg);
    let custom_dialogue = cfg.custom_dialogue.trim();

    let start_instruction = if last_scene_id > 0 {
        format!(
            "Tiếp tục phân tích từ Scene {}. Hãy bỏ qua các scene trước đó.",
            last_scene_id + 1
        )
    } else {
        "Bắt đầu phân tích từ Scene 1 (0s).".to_string()
    };

    let char_action = if char_description.is_empty() {
        String::new()
    } else {
        format!(" và thay thế nhân vật chính bằng: \"{char_description}\"")
    };

    let dialogue_action = if custom_dialogue.is_empty() {
        ", KHÔNG tạo lời thoại tự động".to_string()
    } else {
        format!(", sử dụng lời thoại mới: \"{custom_dialogue}\"")
    };

    let bg_action = if bg_description.is_empty() {
        format!(", TỰ ĐỘNG tạo bối cảnh mới lạ phù hợp phong cách \"{style}\"")
    } else {
        format!(", thay đổi bối cảnh thành: \"{bg_description}\"")
    };

    let style_action = if is_original_style(style) {
        format!("phân tích và giữ nguyên phong cách hình ảnh nguyên bản của video{char_action}{dialogue_action}{bg_action}")
    } else {
        format!("tái hiện lại nội dung video này theo phong cách \"{style}\"{char_action}{dialogue_action}{bg_action}")
    };

    let char_image_instruction = if has_char_image {
        "\n- ẢNH NHÂN VẬT MẪU: Tôi đã đính kèm một hình ảnh nhân vật. Hãy sử dụng ngoại hình từ hình ảnh này để mô tả nhân vật chính (CHAR_1) trong các scene."
    } else {
        ""
    };

    format!(
        r#"{start_instruction} Hãy {style_action}.

LƯU Ý ĐẶC BIỆT:
- KHÔNG tự động trích xuất lời thoại từ video gốc. Nếu không có lời thoại tùy chỉnh, hãy để mảng "dialogue" trống [].
- Tập trung mô tả cực kỳ chi tiết phần "foley_and_ambience" để tạo ra không gian âm thanh sống động.
- Đảm bảo nhân vật CHAR_1 và bối cảnh BACKGROUND_1 đồng nhất 100% trong mọi scene.
- Giọng nói (voice_id) PHẢI là "VOICE_CHAR_1" cho nhân vật CHAR_1 và không được thay đổi trong bất kỳ scene nào.
- Nếu bối cảnh được tạo tự động, nó phải ĐẾN TỪ trí tưởng tượng phong phú của bạn dựa trên phong cách "{style}" nhưng phải khác biệt với video gốc.
- TOÀN BỘ VĂN BẢN MÔ TẢ TRONG JSON PHẢI ĐƯỢC VIẾT BẰNG TIẾNG VIỆT CHUẨN, GIÀU HÌNH ẢNH.

Xuất ra JSON theo cấu trúc sau (ĐẢM BẢO MỖI JSON LÀ 1 DÒNG DUY NHẤT).

Mẫu JSON bắt buộc (VIẾT LIỀN TRÊN 1 DÒNG):
{TEMPLATE}{char_image_instruction}"#,
        TEMPLATE = SCENE_TEMPLATE
    )
}

/// The one-line JSON skeleton the model must fill in. Field order and the
/// `body_metrics` constraint string are load-bearing for Veo 3 consistency.
const SCENE_TEMPLATE: &str = r#"{"scene_id":"[Số]","duration_sec":"8","visual_style":"[Mô tả chi tiết Lighting, Shading, Texture...]","character_lock":{"CHAR_1":{"id":"CHAR_1","name":"[Tên]","species":"[Loài]","gender":"[Giới tính]","age":"[Tuổi]","voice_id":"[ID_GIỌNG_NÓI_CỐ_ĐỊNH]","voice_personality":"[Tính cách]","body_build":"[Dáng]","face_shape":"[Mặt]","hair":"[Tóc]","skin_or_fur_color":"[Màu]","signature_feature":"[Đặc điểm]","outfit_top":"[Áo]","outfit_bottom":"[Quần]","helmet_or_hat":"[Mũ]","shoes_or_footwear":"[Giày]","props":"[Đạo cụ]","body_metrics":"u=cm; abs.height=[Height]; cons=no-auto-rescale,lock-proportions","position":"[Vị trí]","orientation":"[Hướng]","pose":"[Tư thế]","foot_placement":"[Chân]","hand_detail":"[Tay]","expression":"[Biểu cảm]","action_flow":{"pre_action":"[Bắt đầu]","main_action":"[Chính]","post_action":"[Kết thúc]"}}},"background_lock":{"BACKGROUND_1":{"id":"BACKGROUND_1","name":"[Bối cảnh]","setting":"[Indoor/Outdoor]","scenery":"[Mô tả]","props":"[Đồ vật]","lighting":"[Ánh sáng]"}},"camera":{"framing":"[Size]","angle":"[Góc]","movement":"[Chuyển động]","focus":"[Tiêu điểm]"},"foley_and_ambience":{"ambience":["[Âm thanh]"],"fx":["[Hiệu ứng]"],"music":"[Nhạc]"},"audio_markers":{"voice_samples":{"CHAR_1":"[ID_GIỌNG_NÓI_CỐ_ĐỊNH]","NARRATOR":"[NARRATOR_VOICE_ID]"}},"dialogue":[],"lip_sync_director_note":"[Ghi chú]"}"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> CloneConfig {
        CloneConfig {
            style: "Synthwave, neon sunset, 80s retro".into(),
            model: "gemini-3-flash-preview".into(),
            visual_similarity: 100,
            ..Default::default()
        }
    }

    #[test]
    fn full_similarity_is_the_most_deterministic() {
        let c = cfg();
        assert!((temperature_for(&c) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn zero_similarity_is_the_most_creative() {
        let mut c = cfg();
        c.visual_similarity = 0;
        assert!((temperature_for(&c) - 0.8).abs() < 1e-9);
    }

    #[test]
    fn auto_magic_overrides_the_similarity_slider() {
        let mut c = cfg();
        c.visual_similarity = 100;
        c.auto_magic = true;
        assert_eq!(effective_similarity(&c), 0);
        assert!((temperature_for(&c) - 0.8).abs() < 1e-9);
    }

    #[test]
    fn auto_magic_replaces_user_character_and_background_text() {
        let mut c = cfg();
        c.char_description = "một ông già".into();
        c.bg_description = "quán cà phê".into();
        c.auto_magic = true;
        assert!(effective_character(&c).contains("tự động thay đổi ngoại hình"));
        assert!(effective_background(&c).contains("tự do sáng tạo"));
    }

    #[test]
    fn original_style_is_detected_in_both_languages() {
        assert!(is_original_style("Phân tích theo video gốc (Original Style)"));
        assert!(is_original_style("Original Style"));
        assert!(!is_original_style("Dark fantasy, dramatic lighting"));
    }

    #[test]
    fn resuming_asks_for_the_scene_after_the_last_one() {
        let c = cfg();
        assert!(user_prompt(&c, 3, false).starts_with("Tiếp tục phân tích từ Scene 4."));
        assert!(user_prompt(&c, 0, false).starts_with("Bắt đầu phân tích từ Scene 1"));
    }

    #[test]
    fn absent_dialogue_forbids_subtitle_extraction() {
        let c = cfg();
        let sys = system_instruction(&c);
        assert!(sys.contains("KHÔNG TỰ ĐỘNG TẠO PHỤ ĐỀ"));

        let mut c2 = cfg();
        c2.custom_dialogue = "Xin chào".into();
        assert!(system_instruction(&c2).contains("THAY THẾ LỜI THOẠI"));
    }

    #[test]
    fn character_image_note_only_appears_when_an_image_is_attached() {
        let c = cfg();
        assert!(user_prompt(&c, 0, true).contains("ẢNH NHÂN VẬT MẪU"));
        assert!(!user_prompt(&c, 0, false).contains("ẢNH NHÂN VẬT MẪU"));
    }

    #[test]
    fn scene_template_is_a_single_parseable_json_line() {
        assert!(!SCENE_TEMPLATE.contains('\n'));
        serde_json::from_str::<serde_json::Value>(SCENE_TEMPLATE)
            .expect("template must stay valid JSON");
    }
}
