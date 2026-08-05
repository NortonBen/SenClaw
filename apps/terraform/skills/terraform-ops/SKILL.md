---
name: terraform-ops
description: >-
  Vận hành hạ tầng Terraform qua app Terraform: thêm workspace từ thư mục local
  hoặc clone từ git (git tự pull trước khi apply), đọc biến variables.tf, điền và
  lưu file .tfvars (vd prod.tfvars), chạy init/validate/plan/apply/destroy/output
  với console lỗi chi tiết, huỷ run, nhờ AI giải thích lỗi, và cài Terraform CLI
  đa nền tảng nếu máy chưa có. Dùng khi người dùng nói về terraform, tfvars, IaC,
  plan/apply hạ tầng, provision hạ tầng cloud.
triggers:
  - terraform
  - tfvars
  - hạ tầng
  - iac
  - infrastructure as code
  - terraform plan
  - terraform apply
  - terraform destroy
  - terraform init
  - provision
  - deploy hạ tầng
  - biến terraform
  - variables.tf
  - cài terraform
  - workspace terraform
---

# terraform-ops

Dùng MCP server `terraform-mcp` của app **Terraform** (port 4770). Tên tool đầy đủ:
`mcp__terraform-mcp__<tool>` (vd `mcp__terraform-mcp__tf_plan`). App chạy Terraform
CLI thật trên máy user — mọi thao tác đổi hạ tầng phải theo luật an toàn bên dưới.

## Mô hình dữ liệu

- **Workspace** = một dự án Terraform. Hai nguồn:
  - `folder` — thư mục local user chỉ định (app KHÔNG bao giờ xoá thư mục này);
  - `git` — app clone repo về thư mục tự quản (`~/.senclaw/apps/terraform/repos/...`).
    Workspace git có `auto_sync` (mặc định bật): **tự `git pull --ff-only` trước mỗi
    plan/apply/destroy**. Sync tay bằng `tf_sync`.
- **var_file** — file `.tfvars` đã chọn của workspace (vd `prod.tfvars`); plan/apply/destroy
  tự truyền `-var-file=<var_file>`. Đổi bằng `tf_workspace_set` hoặc khi `tf_tfvars_set`.
- **Run** = một lần chạy (init/validate/plan/apply/destroy/output/sync/clone/install).
  Console lưu từng dòng — đọc bằng `tf_run_get` (tham số `after` để đọc tiếp phần mới).

## Luồng chuẩn

1. `tf_status` — xem app + CLI. Nếu `cli.found=false` → hỏi user rồi `tf_cli_install`
   (chạy nền, theo dõi run bằng `tf_run_get`). KHÔNG tự cài khi user chưa đồng ý.
2. Thêm workspace:
   - folder: `tf_workspace_add { source:"folder", path:"/đường/dẫn/tuyệt/đối" }`
   - git: `tf_workspace_add { source:"git", repo_url:"https://…/infra.git", branch:"main", subdir:"deployments/terraform" }`
     → trả `run_id` clone; đợi workspace `status="ready"` (xem `tf_workspace_get`).
   - `subdir` = root Terraform TRONG repo khi *.tf không nằm ở gốc. Nếu `tf_variables`
     trả 0 biến, gọi `GET tf_workspace_get` xem `work_dir`, rồi đặt lại bằng
     `tf_workspace_set { workspace_id, subdir:"…" }` (app tự dò ứng viên trong UI).
   - `tf_open_dir { workspace_id }` mở thư mục workspace (bản clone) trong Finder/Explorer.
3. `tf_variables { workspace_id }` — đọc biến (name/type/default/sensitive) đúng như
   form Apply trong UI. `tf_tfvars_get` đọc giá trị file tfvars đang chọn.
4. Điền biến: `tf_tfvars_set { workspace_id, file:"prod.tfvars", values:{ region:"ap-southeast-1", instance_count:2 } }`
   — mặc định MERGE vào file; `replace:true` mới ghi đè cả file. Giá trị là JSON đúng kiểu
   (number là số, không phải chuỗi; list/map là JSON thật).
5. `tf_plan { workspace_id }` — tự init nếu chưa, git pull nếu là workspace git.
   Đọc `console_tail` trả về để tóm tắt thay đổi (`+ create / ~ update / - destroy`).
6. Muốn apply: **báo cáo plan cho user và CHỜ user đồng ý rõ ràng** rồi mới
   `tf_apply { workspace_id, confirm:true }`. Tương tự `tf_destroy` — nguy hiểm hơn nữa.

## Luật an toàn (bắt buộc)

- `tf_apply` / `tf_destroy` **thay đổi hạ tầng thật** (`-auto-approve`). CHỈ gọi khi user
  vừa yêu cầu/đồng ý rõ ràng trong hội thoại; luôn chạy `tf_plan` trước và tóm tắt cho
  user thấy sẽ đổi gì. Không bao giờ tự ý destroy.
- `tf_workspace_delete` cần `confirm:true`; nguồn git xoá luôn bản clone, nguồn folder
  không đụng thư mục user.
- Biến `sensitive` (mật khẩu, key): KHÔNG in giá trị ra chat; khi user đưa giá trị thì
  ghi thẳng vào tfvars bằng `tf_tfvars_set` rồi xác nhận đã lưu.
- Run kẹt/lâu: `tf_run_cancel { run_id }`. Lỗi khó hiểu: `tf_ai_explain { run_id }`.

## Ví dụ gọi nhanh

```
tf_workspace_add { "source": "git", "repo_url": "https://github.com/acme/infra.git", "branch": "main" }
tf_variables    { "workspace_id": 1 }
tf_tfvars_set   { "workspace_id": 1, "file": "prod.tfvars", "values": { "region": "ap-southeast-1" } }
tf_plan         { "workspace_id": 1 }
tf_apply        { "workspace_id": 1, "confirm": true }   # chỉ sau khi user đồng ý
tf_run_get      { "run_id": 12, "after": 0 }
```
