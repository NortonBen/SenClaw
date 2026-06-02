import { useState } from 'react';
import { Modal, Form, Input, Button, Typography, theme, App } from 'antd';
import { SendOutlined, RobotOutlined, ReloadOutlined } from '@ant-design/icons';
import { api } from '../api';

const { TextArea } = Input;
const { Text } = Typography;

interface Props {
  open: boolean;
  onClose: () => void;
  onSend: (to: string, subject: string, body: string) => Promise<void>;
  initialTo?: string;
  initialSubject?: string;
}

export function ComposeModal({ open, onClose, onSend, initialTo = '', initialSubject = '' }: Props) {
  const { token } = theme.useToken();
  const { message } = App.useApp();
  const [form] = Form.useForm();
  const [sending, setSending] = useState(false);
  const [aiDrafting, setAiDrafting] = useState(false);
  const [aiPrompt, setAiPrompt] = useState('');
  const [showAiPrompt, setShowAiPrompt] = useState(false);
  const [draft, setDraft] = useState<{ to: string; subject: string; body: string } | null>(null);

  const handleAiDraft = async () => {
    if (!aiPrompt.trim()) return;
    setAiDrafting(true);
    try {
      const data = await api.draft(aiPrompt);
      form.setFieldsValue({ subject: data.subject, body: data.body });
      setShowAiPrompt(false);
      setAiPrompt('');
      message.success('AI đã soạn thảo xong');
    } catch {
      message.error('Không thể tạo bản nháp');
    } finally {
      setAiDrafting(false);
    }
  };

  const handlePreview = async () => {
    try {
      const vals = await form.validateFields();
      setDraft({ to: vals.to, subject: vals.subject, body: vals.body });
    } catch { /* validation errors shown inline */ }
  };

  const handleSend = async () => {
    if (!draft) return;
    setSending(true);
    try {
      await onSend(draft.to, draft.subject, draft.body);
      message.success(`Email đã gửi tới ${draft.to}`);
      form.resetFields();
      setDraft(null);
      onClose();
    } catch (e) {
      message.error(e instanceof Error ? e.message : 'Gửi email thất bại');
    } finally {
      setSending(false);
    }
  };

  const handleClose = () => {
    form.resetFields();
    setDraft(null);
    setShowAiPrompt(false);
    onClose();
  };

  return (
    <Modal
      title="Soạn Email"
      open={open}
      onCancel={handleClose}
      width={600}
      footer={
        draft ? (
          <div className="flex justify-between items-center">
            <Button onClick={() => setDraft(null)} icon={<ReloadOutlined />}>
              Chỉnh sửa lại
            </Button>
            <Button type="primary" icon={<SendOutlined />} loading={sending} onClick={handleSend}>
              Xác nhận gửi
            </Button>
          </div>
        ) : (
          <div className="flex justify-between items-center">
            <Button icon={<RobotOutlined />} onClick={() => setShowAiPrompt(v => !v)}>
              AI soạn thảo
            </Button>
            <div className="flex gap-2">
              <Button onClick={handleClose}>Hủy</Button>
              <Button type="primary" onClick={handlePreview}>
                Xem trước &amp; Gửi
              </Button>
            </div>
          </div>
        )
      }
    >
      {showAiPrompt && !draft && (
        <div
          className="mb-3 p-3 rounded border"
          style={{ borderColor: token.colorPrimary, background: token.colorPrimaryBg }}
        >
          <Text type="secondary" className="text-xs mb-1 block">
            Mô tả nội dung email bạn muốn gửi, AI sẽ soạn thảo:
          </Text>
          <div className="flex gap-2">
            <Input
              placeholder="VD: Xin lỗi vì trễ deadline dự án ABC..."
              value={aiPrompt}
              onChange={e => setAiPrompt(e.target.value)}
              onPressEnter={handleAiDraft}
              disabled={aiDrafting}
            />
            <Button type="primary" size="small" loading={aiDrafting} onClick={handleAiDraft}>
              Soạn
            </Button>
          </div>
        </div>
      )}

      {draft ? (
        <div className="space-y-3">
          <div className="p-3 rounded border" style={{ borderColor: token.colorBorderSecondary }}>
            <div className="mb-2">
              <Text type="secondary" className="text-xs">Đến:</Text>
              <Text strong className="ml-2">{draft.to}</Text>
            </div>
            <div className="mb-2">
              <Text type="secondary" className="text-xs">Chủ đề:</Text>
              <Text strong className="ml-2">{draft.subject}</Text>
            </div>
            <div
              className="mt-3 pt-3 border-t whitespace-pre-wrap text-sm"
              style={{ borderColor: token.colorBorderSecondary, color: token.colorText }}
            >
              {draft.body}
            </div>
          </div>
          <div
            className="p-2 rounded text-xs text-center"
            style={{ background: token.colorWarningBg, color: token.colorWarningText }}
          >
            Kiểm tra kỹ trước khi gửi. Nhấn "Xác nhận gửi" để gửi email này.
          </div>
        </div>
      ) : (
        <Form
          form={form}
          layout="vertical"
          initialValues={{ to: initialTo, subject: initialSubject }}
          className="mt-2"
        >
          <Form.Item name="to" label="Đến" rules={[{ required: true, type: 'email', message: 'Nhập email hợp lệ' }]}>
            <Input placeholder="recipient@example.com" />
          </Form.Item>
          <Form.Item name="subject" label="Chủ đề" rules={[{ required: true, message: 'Nhập chủ đề' }]}>
            <Input placeholder="Chủ đề email..." />
          </Form.Item>
          <Form.Item name="body" label="Nội dung" rules={[{ required: true, message: 'Nhập nội dung' }]}>
            <TextArea rows={8} placeholder="Nội dung email..." />
          </Form.Item>
        </Form>
      )}
    </Modal>
  );
}
