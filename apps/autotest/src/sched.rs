//! Vòng lặp lịch chạy định kỳ: mỗi 30s quét `schedules` đến hạn và chạy suite
//! với trigger `schedule`. Chỉ MỘT suite chạy tại một thời điểm cho mỗi tick —
//! chạy tuần tự để hai suite nặng không giẫm nhau; suite đến hạn trong lúc đó
//! sẽ được tick sau nhặt (last_run_at chỉ set khi thật sự chạy).

use crate::api::AppState;
use crate::db;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        // Run 'running' mồ côi từ lần chạy trước (app bị kill giữa chừng).
        state.db.reap_stale_runs();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let now = db::now();
            for suite_id in state.db.due_schedules(now) {
                state.db.schedule_touch(suite_id, now);
                match state.runner.run_suite(suite_id, None, "schedule").await {
                    Ok(run_id) => {
                        let _ = run_id;
                    }
                    Err(e) => {
                        state.db.log(
                            "schedule",
                            &format!("lịch chạy suite #{suite_id} lỗi: {e}"),
                            "",
                        );
                    }
                }
            }
        }
    });
}
