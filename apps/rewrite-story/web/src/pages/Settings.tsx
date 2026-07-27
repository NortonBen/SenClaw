import { useEffect, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  App as AntApp,
  Button,
  Card,
  Divider,
  Form,
  Input,
  InputNumber,
  Skeleton,
  Space,
  Typography,
} from "antd";
import { SaveOutlined } from "@ant-design/icons";

import { api } from "../lib/api";

const { Title, Paragraph } = Typography;

type SettingsForm = Record<string, string | number>;

export default function Settings() {
  const qc = useQueryClient();
  const { message } = AntApp.useApp();
  const [form] = Form.useForm<SettingsForm>();

  const settings = useQuery({ queryKey: ["settings"], queryFn: api.getSettings });

  // Populate once. `settings.data` gets a fresh object identity on every
  // background refetch (window focus, after staleTime), and re-running
  // setFieldsValue then would wipe whatever the user was mid-way through typing.
  const populated = useRef(false);
  useEffect(() => {
    if (settings.data && !populated.current) {
      populated.current = true;
      form.setFieldsValue(settings.data);
    }
  }, [settings.data, form]);

  const save = useMutation({
    mutationFn: (v: SettingsForm) =>
      api.putSettings(
        Object.fromEntries(Object.entries(v).map(([k, val]) => [k, String(val)]))
      ),
    onSuccess: () => {
      message.success("Đã lưu cấu hình");
      qc.invalidateQueries({ queryKey: ["settings"] });
    },
    onError: (e: Error) => message.error(e.message),
  });

  if (settings.isLoading) return <Skeleton active />;

  const field = { style: { width: 220 } };

  return (
    <>
      <Title level={3} style={{ marginTop: 0, marginBottom: 20 }}>
        Cấu hình
      </Title>

      <Form form={form} layout="vertical" onFinish={(v) => save.mutate(v)}>
        <Space orientation="vertical" size={16} style={{ width: "100%" }}>
          <Card title="Chia chunk">
            <Alert
              style={{ marginBottom: 16 }}
              type="info"
              showIcon
              title="Đổi kích thước chunk không cắt lại truyện cũ"
              description="Chunk được lưu theo truyện ngay lần cắt đầu tiên. Cấu hình mới chỉ áp dụng cho truyện chưa từng được viết lại."
            />
            <Form.Item
              name="hybrid_split_min_size"
              label="Kích thước chunk tối thiểu (ký tự)"
              tooltip="Chunk chỉ được phép ngắt theo ngữ nghĩa sau khi vượt ngưỡng này."
            >
              <InputNumber min={100} max={100000} {...field} />
            </Form.Item>
            <Form.Item
              name="hybrid_split_max_size"
              label="Kích thước chunk tối đa (ký tự)"
              tooltip="Model trả về một lượng văn gần như cố định mỗi lần gọi; chunk vượt trần đó sẽ bị tóm tắt thay vì viết lại. Để nhỏ (~2000)."
            >
              <InputNumber min={200} max={200000} {...field} />
            </Form.Item>
            <Form.Item
              name="hybrid_split_threshold"
              label="Ngưỡng chuyển cảnh (0–1)"
              tooltip="Độ tương đồng dưới ngưỡng này được coi là chuyển cảnh và sẽ ngắt chunk. Cao hơn = cắt dày hơn."
            >
              <InputNumber min={0} max={1} step={0.05} {...field} />
            </Form.Item>
          </Card>

          <Card title="Viết lại">
            <Form.Item name="default_creativity_ratio" label="Mức sáng tạo mặc định (0–100)">
              <InputNumber min={0} max={100} {...field} />
            </Form.Item>
            <Form.Item name="default_length_variance" label="Dung sai độ dài mặc định (%)">
              <InputNumber min={0} max={100} {...field} />
            </Form.Item>
            <Form.Item
              name="max_output_tokens"
              label="Ngân sách token đầu ra mỗi chunk"
              tooltip="Độ dài bản viết lại tỉ lệ gần như tuyến tính với giá trị này trên bridge của SenClaw — đặt thấp thì model trả về bản TÓM TẮT chứ không phải bản viết lại, và không có lỗi nào báo."
              extra="Nếu bản viết lại ngắn hơn nhiều so với bản gốc, hãy tăng giá trị này trước tiên."
            >
              <InputNumber min={2048} max={200000} step={1000} {...field} />
            </Form.Item>
          </Card>

          <Card title="Hiệu năng & model">
            <Form.Item
              name="max_concurrent_processes"
              label="Số truyện chạy song song"
              tooltip="Mỗi tiến trình gọi model liên tục; để thấp nếu bị giới hạn tốc độ."
            >
              <InputNumber min={1} max={8} {...field} />
            </Form.Item>
            <Form.Item
              name="parallel_chunks"
              label="Số chunk viết song song trong một truyện"
              tooltip="1 = tuần tự, mỗi chunk nối tiếp đuôi bản đã viết lại của chunk trước — mạch văn mượt nhất. Lớn hơn 1 thì các chunk cùng lô dùng đuôi bản gốc làm cầu nối: nhanh hơn gần tuyến tính, đổi lại mối nối kém mượt hơn một chút."
              extra="Truyện vài trăm chunk: đặt 3–4 để rút thời gian từ hàng giờ xuống dưới một giờ."
            >
              <InputNumber min={1} max={8} {...field} />
            </Form.Item>
            <Form.Item
              name="llm_profile"
              label="Profile LLM của SenClaw"
              tooltip="Bỏ trống để dùng model đang bật của daemon."
            >
              <Input placeholder="(theo model đang bật)" style={{ width: 320 }} allowClear />
            </Form.Item>
            <Divider style={{ margin: "8px 0 16px" }} />
            <Paragraph type="secondary" style={{ margin: 0 }}>
              App không giữ API key nào — mọi lời gọi model đều đi qua LLM chung của
              SenClaw.
            </Paragraph>
          </Card>

          <Button
            type="primary"
            size="large"
            htmlType="submit"
            icon={<SaveOutlined />}
            loading={save.isPending}
          >
            Lưu cấu hình
          </Button>
        </Space>
      </Form>
    </>
  );
}
