---
name: crm-organizations
description: >-
  Accounts and the sellable catalogue in the SenClaw CRM: organizations
  (direct_customer, affiliated_company, partner, supplier, prospect), which
  contacts belong to them, the service + hardware catalogue with its pricing
  model, and the line items that price a deal. Use for "công ty nào", "danh sách
  tổ chức", "khách doanh nghiệp", "thêm công ty", "X làm ở công ty Y", "gán
  khách vào công ty", "ai làm ở công ty X", "công ty X có deal gì", "bảng giá",
  "bên mình bán gì", "danh sách dịch vụ", "thêm dịch vụ", "thêm sản phẩm", "tạo
  gói", "đổi giá dịch vụ", "ngừng bán dịch vụ", "thêm dịch vụ vào deal", "deal
  này gồm những gì", "doanh thu theo công ty", "bán dịch vụ hay phần cứng nhiều
  hơn", "list organizations", "link contact to company", "price list", "add a
  service", "deal line items", "revenue by organization". NOT for the person's
  own profile or interaction log (use crm-quick-lookup / crm-log-interaction).
---

# crm-organizations

Manage **accounts** and the **sellable catalogue** via the **`crm-mcp`** server.

## The model in one screen

- **Organization** = an account. A contact belongs to **0..n** organizations
  through the `customer_organizations` join, **one of them flagged primary**.
  The primary link also syncs the contact's legacy `customers.company` text —
  so a contact's `company` field is a *projection* of their primary org, not an
  independent fact. Change it by re-linking, not by editing the text.
  Kinds: `direct_customer | affiliated_company | partner | supplier | prospect`.
- **Service** = a catalogue entry — something you sell. `kind`:
  `service | hardware`. Carries `amount`, `currency`, and a `pricing_model`:
  `fixed | hourly | daily | monthly | yearly`.
- **Line item** = a service attached to a deal (`deal_services`), with a
  `quantity` and a **frozen** `unit_amount`. **A deal's `amount` is recomputed
  from its line items when it has any.** Editing the catalogue price afterwards
  does not silently re-price a deal that was already quoted.

## Tool catalogue — 18 tools

### Organizations — read

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__crm_list_organizations`** | `q`, `kind`, `limit` (200, max 500) | List/search. Returns id, name, kind, website, domain, industry + contact/deal counts + open pipeline value. |
| **`mcp__crm-mcp__crm_get_organization`** | `id` | One org **with its contacts and its deals**. Start here for "ai làm ở công ty X". |
| **`mcp__crm-mcp__crm_find_organization`** | `name` | Resolve by exact name (case-insensitive). Returns `{found, id}`. |
| **`mcp__crm-mcp__crm_customer_organizations`** | `customer_id` | Which orgs a contact belongs to, **primary first**. |

### Organizations — write

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__crm_create_organization`** | `name` (required), `kind` (default `direct_customer`), `website`, `domain`, `industry`, `size`, `address`, `notes`, `tags` | Create. **Resolve first** — see the rule below. |
| **`mcp__crm-mcp__crm_update_organization`** | `id` + any field | Patch. Only what you pass changes. |
| **`mcp__crm-mcp__crm_delete_organization`** | `id` | Contacts and deals are **UNLINKED, not deleted**. Confirm first. |
| **`mcp__crm-mcp__crm_link_organization`** | `customer_id` (required), `organization_id` **or** `organization_name`, `role_title`, `is_primary` | Link a contact to an org. `organization_name` resolves-or-creates. `is_primary: true` also updates the contact's `company`. |
| **`mcp__crm-mcp__crm_unlink_organization`** | `customer_id`, `organization_id` | Remove the link. Neither record is deleted. |

### Catalogue — read

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__crm_list_services`** | `q`, `kind` (`service\|hardware`), `active_only`, `limit` (200, max 500) | The price list. Returns name, kind, amount, currency, pricing_model + how many deals use each. |
| **`mcp__crm-mcp__crm_get_service`** | `id` | One catalogue entry. |
| **`mcp__crm-mcp__crm_deal_services`** | `deal_id` | A deal's line items with quantity, unit amount, line totals and the deal total. |
| **`mcp__crm-mcp__crm_revenue_breakdown`** | `limit` (top N orgs, default 20) | Deal value by organization, deal value by service kind, org counts by type. |

### Catalogue — write

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__crm_create_service`** | `name` (required), `kind` (default `service`), `amount`, `currency` (default VND), `pricing_model` (default `fixed`), `unit`, `sku`, `description` | Add an entry. |
| **`mcp__crm-mcp__crm_update_service`** | `id` + any field, incl. `active` | Patch. **`active: false` retires an entry** without deleting it. |
| **`mcp__crm-mcp__crm_delete_service`** | `id` | **FAILS if it prices any deal.** Deactivate instead. |
| **`mcp__crm-mcp__crm_attach_service`** | `deal_id`, `service_id`, `quantity` (default 1), `unit_amount`, `note` | Add a line item. `unit_amount` defaults to the current catalogue price and is then **frozen** on the line. Recomputes the deal total. |
| **`mcp__crm-mcp__crm_detach_service`** | `deal_id`, `service_id` | Remove a line item. Recomputes the deal total. |

## Hard rules

### 1. Resolve before you create — never duplicate an account

**Always call `crm_find_organization({name})` before
`crm_create_organization`.** Two rows for the same company is the failure mode
this whole surface exists to prevent: it splits the contacts, splits the deals,
and quietly halves every number in `crm_revenue_breakdown`. Nothing merges them
back for you.

- Found → use that `id`.
- Not found → check the near-misses too. `crm_list_organizations({q: "…"})`
  catches "Shop Co" vs "Shop Co., Ltd" vs "shopco" that an exact-name lookup
  misses. If a plausible match turns up, **ask the user** rather than creating.
- Genuinely new → create.

`crm_link_organization({organization_name})` does resolve-or-create for you —
convenient, but it resolves by **exact name only**, so a typo mints a second
account. When the name came from a human sentence rather than from a prior
lookup, resolve it yourself first and pass `organization_id`.

### 2. Deactivate; do not delete a service that priced a deal

`crm_delete_service` **fails** when the entry appears on any deal, and that
guard is protecting history: those line items are what a customer was quoted.
Retire with `crm_update_service({id, active: false})`. It disappears from new
quoting; deals already priced with it keep their line item intact.

If a user asks to "xoá dịch vụ" / "delete a service", check
`crm_list_services` for its deal count first, and offer deactivation when it is
in use. Do not report a failed delete as a mysterious error.

### 3. Price changes are not retroactive — and that is deliberate

`unit_amount` is frozen on the line at attach time. Changing the catalogue price
does **not** re-price existing deals. To re-quote an existing deal, detach and
re-attach the line (or pass an explicit `unit_amount`) — and say so, because it
changes what the customer was quoted.

### 4. One primary org

`is_primary: true` also rewrites the contact's `company` text. Do not fight it
by editing `company` through `crm_update_customer` — re-link instead, or the two
will disagree and the CRM will show a company the contact is not linked to.

## Composed workflows

**"Anna làm ở Shop Co"**
1. `crm_find_organization({name: "Shop Co"})` → not found?
   `crm_list_organizations({q: "shop"})` to catch near-misses.
2. Confirm with the user if anything close turns up.
3. `crm_link_organization({customer_id: anna.id, organization_id: shop.id, role_title: "Marketing Lead", is_primary: true})`

**"Deal này gồm gói yearly + 2 máy"**
1. `crm_list_services({q: "yearly"})` → id. Same for the hardware.
2. `crm_attach_service({deal_id, service_id: yearly.id, quantity: 1})`
3. `crm_attach_service({deal_id, service_id: device.id, quantity: 2})`
4. `crm_deal_services({deal_id})` → read back the recomputed total and report it.

**"Doanh thu theo công ty"**
1. `crm_revenue_breakdown({limit: 10})` → `byOrganization`, `byServiceKind`,
   `organizationsByKind`.
2. Name the top orgs and the service-vs-hardware split. If a company looks
   split across two near-identical rows, flag it — that is a duplicate account.

## Do not

- Do not create an organization without resolving first.
- Do not delete an org or a service without an explicit confirmation — show what
  will be unlinked or lost.
- Do not invent a price. The catalogue is the source of truth; if it is not
  there, say "chưa có trong bảng giá" and ask.
- Do not quote a figure to a customer from here. Outbound goes through
  `sale_send` and its guardrail (**`crm-sale-followup`**), which deliberately
  queues price wording for a human.

## Style

- Reply in the user's language (default Vietnamese).
- Include ids when disambiguation matters: "#4 Shop Co (direct_customer)".
- Format money with its currency, and say the pricing model when it is not
  `fixed` — "12.000.000 VND/năm" reads very differently from "12.000.000 VND".
