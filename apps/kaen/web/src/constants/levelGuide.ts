export type LevelKey = 'level0' | 'level1' | 'level2' | 'level3' | 'level4' | 'level5' | 'level6Plus';

export interface LevelGuideItem {
  key: LevelKey;
  label: string;
  color: string;
  interval: string;
  description: string;
  showInProfile: boolean;
}

export const LEVEL_GUIDE: LevelGuideItem[] = [
  {
    key: 'level0',
    label: 'Level 0 (Mới)',
    color: '#ef4444',
    interval: 'Ngay sau khi học',
    description: 'Từ chưa học hoặc vừa gặp lần đầu. Cần ôn ngay để chuyển sang trí nhớ ngắn hạn.',
    showInProfile: false,
  },
  {
    key: 'level1',
    label: 'Level 1',
    color: '#f97316',
    interval: 'Sau 30 phút',
    description: 'Nhắc lại cực nhanh để não không quên ngay. Giúp hình thành vết nhớ đầu tiên.',
    showInProfile: true,
  },
  {
    key: 'level2',
    label: 'Level 2',
    color: '#f59e0b',
    interval: 'Sau 1 ngày',
    description: 'Active Recall ngày hôm sau để kiểm tra lại và chuyển sang trí nhớ trung hạn.',
    showInProfile: true,
  },
  {
    key: 'level3',
    label: 'Level 3',
    color: '#eab308',
    interval: 'Sau 3 ngày',
    description: 'Củng cố sau 72 giờ, bắt đầu chuyển sang luyện gõ để kiểm tra kỹ hơn.',
    showInProfile: true,
  },
  {
    key: 'level4',
    label: 'Level 4',
    color: '#84cc16',
    interval: 'Sau 1 tuần',
    description: 'Tăng khoảng cách lên 7 ngày để xác nhận từ đã nằm trong trí nhớ dài hạn.',
    showInProfile: true,
  },
  {
    key: 'level5',
    label: 'Level 5',
    color: '#22c55e',
    interval: 'Sau 1 tháng',
    description: 'Giữ nhịp ôn định kỳ hằng tháng để đảm bảo không bị rơi mất.',
    showInProfile: true,
  },
  {
    key: 'level6Plus',
    label: 'Level 6+',
    color: '#10b981',
    interval: 'Sau 3 tháng',
    description: 'Chỉ cần nhắc lại mỗi quý để duy trì sự quen thuộc và phản xạ tự nhiên.',
    showInProfile: true,
  },
];

