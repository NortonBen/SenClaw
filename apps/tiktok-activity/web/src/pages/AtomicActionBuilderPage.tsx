import { useCallback, useEffect, useRef, useState } from "react";
import { Button, Card, Checkbox, Input, Modal, Select, Space, Spin, Typography, message } from "antd";
import { Link, useSearchParams } from "react-router-dom";
import { CopyOutlined, DeleteOutlined, PlusOutlined, SaveOutlined, UploadOutlined } from "@ant-design/icons";
import { api } from "../api";
import type { FlowAction, FlowAtomic, SavedFlowAction } from "../types/api";
import { AtomicChainEditor } from "./flows/AtomicChainEditor";
import { StepParamsEditor } from "./flows/StepParamsEditor";
import { ui } from "./flows/constants";
import { parseAtomicsImportPayload } from "./flows/flowIo";

const STORAGE_KEY = "tiktok-activity.atomic-action-builder";

const PRESET_FLOW_SEARCH_FOLLOW_URL = "/presets/tiktok-search-follow-first-user.flow.json";
const PRESET_FLOW_OPEN_USER_AFTER_FOLLOW_URL = "/presets/tiktok-open-user-after-follow.flow.json";
const PRESET_ATOMICS_PROFILE_OPEN_FIRST_POST_URL = "/presets/tiktok-profile-open-first-post.atomics.json";

function newImportAtomicId(i: number): string {
  return `atom_import_${i}_${Date.now()}_${Math.random().toString(16).slice(2, 10)}`;
}

type Draft = {
  actionName: string;
  stepParams: Record<string, string>;
  atomics: FlowAtomic[];
  timeoutSeconds: number;
};
type AccountPick = { id: string; username?: string };
type AtomicActionAnalyzeResult = {
  pageUrl: string;
  pageTitle: string;
  outlineChars: number;
  interactiveCount: number;
  outlineTruncated: boolean;
  selectorMapPreview?: string;
  suggestedName: string;
  suggestedStepParams?: Record<string, string>;
  suggestedAtomics?: FlowAtomic[];
  suggestedAtomicsJson?: string;
  creationFlow?: string;
  probeGoal?: string;
  probeMaxSteps?: number;
  probeStepsLogged?: number;
  probeLastSummary?: string;
  probeRunError?: string;
};

function loadDraft(): Draft | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const p = JSON.parse(raw) as Draft & { timeoutSeconds?: number };
    if (!p || typeof p.actionName !== "string" || !Array.isArray(p.atomics)) return null;
    return {
      actionName: p.actionName,
      stepParams: p.stepParams && typeof p.stepParams === "object" ? p.stepParams : {},
      atomics: p.atomics,
      timeoutSeconds: typeof p.timeoutSeconds === "number" && Number.isFinite(p.timeoutSeconds) ? p.timeoutSeconds : 60,
    };
  } catch {
    return null;
  }
}

function saveDraft(d: Draft) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(d));
  } catch {
    // ignore quota
  }
}

export function AtomicActionBuilderPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const savedId = searchParams.get("id");

  const [actionName, setActionName] = useState("");
  const [stepParams, setStepParams] = useState<Record<string, string>>({});
  const [atomics, setAtomics] = useState<FlowAtomic[]>([]);
  const [timeoutSeconds, setTimeoutSeconds] = useState(60);
  /** id step trong payload khi đã lưu server (ổn định giữa các lần Lưu). */
  const [serverStepId, setServerStepId] = useState("");
  const [hydrated, setHydrated] = useState(false);
  const [loadingSaved, setLoadingSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const [savedList, setSavedList] = useState<SavedFlowAction[]>([]);
  const [accounts, setAccounts] = useState<AccountPick[]>([]);
  const [analyzeBusy, setAnalyzeBusy] = useState(false);
  const [analyzeAccountId, setAnalyzeAccountId] = useState("");
  const [analyzeURL, setAnalyzeURL] = useState("");
  const [analyzeGoal, setAnalyzeGoal] = useState("");
  const [analyzeIntent, setAnalyzeIntent] = useState("");
  const [analyzeOutlineOnly, setAnalyzeOutlineOnly] = useState(false);
  const [analyzeResult, setAnalyzeResult] = useState<AtomicActionAnalyzeResult | null>(null);
  const jsonFileInputRef = useRef<HTMLInputElement>(null);

  const refreshSavedList = useCallback(async () => {
    try {
      const list = await api<SavedFlowAction[] | null>("/api/saved-flow-actions");
      setSavedList(Array.isArray(list) ? list : []);
    } catch {
      setSavedList([]);
    }
  }, []);

  useEffect(() => {
    const d = loadDraft();
    if (d) {
      setActionName(d.actionName);
      setStepParams(d.stepParams);
      setAtomics(d.atomics);
      setTimeoutSeconds(d.timeoutSeconds);
    }
    setHydrated(true);
    void refreshSavedList();
  }, [refreshSavedList]);

  useEffect(() => {
    void (async () => {
      try {
        const r = await api<{ items?: AccountPick[] }>("/api/accounts?pageSize=300&page=1");
        setAccounts(Array.isArray(r.items) ? r.items : []);
      } catch {
        setAccounts([]);
      }
    })();
  }, []);

  useEffect(() => {
    if (!hydrated) return;
    saveDraft({ actionName, stepParams, atomics, timeoutSeconds });
  }, [hydrated, actionName, stepParams, atomics, timeoutSeconds]);

  const applyFromSaved = useCallback((s: SavedFlowAction) => {
    setActionName(s.name);
    setStepParams(s.step.params ?? {});
    setAtomics(s.step.atomics ?? []);
    setTimeoutSeconds(s.step.timeoutSeconds > 0 ? s.step.timeoutSeconds : 60);
    setServerStepId(s.step.id || "");
  }, []);

  useEffect(() => {
    if (!hydrated || !savedId) {
      if (hydrated && !savedId) {
        setServerStepId("");
      }
      return;
    }
    let cancelled = false;
    void (async () => {
      setLoadingSaved(true);
      try {
        const s = await api<SavedFlowAction>(`/api/saved-flow-actions/${encodeURIComponent(savedId)}`);
        if (cancelled) return;
        applyFromSaved(s);
        message.success("Đã tải action từ server");
      } catch {
        if (!cancelled) {
          message.error("Không tải được action đã lưu");
          setSearchParams({}, { replace: true });
        }
      } finally {
        if (!cancelled) setLoadingSaved(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [hydrated, savedId, applyFromSaved, setSearchParams]);

  const buildStepSnippet = useCallback((): FlowAction => {
    const name = actionName.trim() || "Playwright atomics (builder)";
    const sid = serverStepId.trim() || `step_builder_${Date.now()}`;
    return {
      id: sid,
      type: "playwright_atomics",
      name,
      config: {},
      timeoutSeconds,
      params: { ...stepParams },
      atomics: atomics.map((a) => ({
        id: a.id,
        name: a.name,
        kind: a.kind,
        params: { ...(a.params ?? {}) },
      })),
    };
  }, [actionName, atomics, serverStepId, stepParams, timeoutSeconds]);

  const copyStepJson = useCallback(() => {
    const step = buildStepSnippet();
    const s = JSON.stringify(step, null, 2);
    void navigator.clipboard.writeText(s).then(
      () => message.success("Đã copy JSON một step (playwright_atomics)"),
      () => {
        message.warning("Không copy được clipboard");
        console.info(s);
      }
    );
  }, [buildStepSnippet]);

  const copyAtomicsOnly = useCallback(() => {
    const s = JSON.stringify({ atomics }, null, 2);
    void navigator.clipboard.writeText(s).then(
      () => message.success("Đã copy { atomics: [...] }"),
      () => message.warning("Không copy được")
    );
  }, [atomics]);

  const clearAll = useCallback(() => {
    setActionName("");
    setStepParams({});
    setAtomics([]);
    setTimeoutSeconds(60);
    setServerStepId("");
    setSearchParams({}, { replace: true });
    try {
      localStorage.removeItem(STORAGE_KEY);
    } catch {
      /* empty */
    }
    message.info("Đã xóa nháp và chọn action mới");
  }, [setSearchParams]);

  const saveToServer = useCallback(async () => {
    const name = actionName.trim();
    if (!name) {
      message.error("Nhập tên action trước khi lưu");
      return;
    }
    try {
      setSaving(true);
      const step = buildStepSnippet();
      const payload = {
        id: savedId ?? "",
        name,
        step,
      };
      const saved = await api<SavedFlowAction>("/api/saved-flow-actions", "POST", payload);
      setSearchParams({ id: saved.id }, { replace: true });
      setServerStepId(saved.step.id);
      await refreshSavedList();
      message.success(savedId ? "Đã cập nhật action trên server" : "Đã tạo action trên server");
    } catch (e) {
      message.error(String(e));
    } finally {
      setSaving(false);
    }
  }, [actionName, buildStepSnippet, refreshSavedList, savedId, setSearchParams]);

  const applyAtomicsJsonText = useCallback(
    (text: string, label: string) => {
      try {
        const { atomics: rows, flowName, sourceHint } = parseAtomicsImportPayload(text);
        const normalized = rows.map((a, i) => ({
          id: a.id || newImportAtomicId(i),
          name: a.name,
          kind: a.kind,
          params: { ...(a.params ?? {}) },
        }));
        setAtomics(normalized);
        const nameTrim = actionName.trim();
        if (flowName?.trim() && !nameTrim) {
          setActionName(flowName.trim());
        }
        const extra = [sourceHint, flowName].filter(Boolean).join(" — ");
        message.success(extra ? `Đã import ${label} (${extra})` : `Đã import ${label}`);
      } catch (e) {
        message.error(e instanceof Error ? e.message : "JSON không hợp lệ");
      }
    },
    [actionName]
  );

  const loadPresetUrl = useCallback(
    async (url: string, label: string) => {
      try {
        const res = await fetch(url);
        if (!res.ok) {
          throw new Error(`HTTP ${res.status}`);
        }
        const text = await res.text();
        applyAtomicsJsonText(text, label);
      } catch (e) {
        message.error(e instanceof Error ? e.message : "Không tải được preset");
      }
    },
    [applyAtomicsJsonText]
  );

  const deleteFromServer = useCallback(() => {
    if (!savedId) {
      message.info("Chưa có bản ghi trên server để xóa");
      return;
    }
    Modal.confirm({
      title: "Xóa action đã lưu?",
      content: "Thao tác không hoàn tác.",
      okText: "Xóa",
      okType: "danger",
      cancelText: "Hủy",
      onOk: async () => {
        try {
          await api(`/api/saved-flow-actions/${encodeURIComponent(savedId)}`, "DELETE");
          message.success("Đã xóa");
          clearAll();
          await refreshSavedList();
        } catch (e) {
          message.error(String(e));
        }
      },
    });
  }, [clearAll, refreshSavedList, savedId]);

  const runAnalyzeAtomic = useCallback(async () => {
    if (!analyzeAccountId.trim()) {
      message.warning("Chọn account trước khi chạy AI");
      return;
    }
    if (!analyzeURL.trim()) {
      message.warning("Nhập URL cần phân tích");
      return;
    }
    setAnalyzeBusy(true);
    setAnalyzeResult(null);
    try {
      const out = await api<AtomicActionAnalyzeResult>("/api/saved-flow-actions/analyze-page", "POST", {
        accountId: analyzeAccountId.trim(),
        url: analyzeURL.trim(),
        goal: analyzeGoal.trim(),
        actionIntent: analyzeIntent.trim(),
        outlineOnly: analyzeOutlineOnly,
      });
      setAnalyzeResult(out);
      if (out.suggestedName?.trim()) setActionName(out.suggestedName.trim());
      if (out.suggestedStepParams && Object.keys(out.suggestedStepParams).length > 0) setStepParams(out.suggestedStepParams);
      if (Array.isArray(out.suggestedAtomics) && out.suggestedAtomics.length > 0) {
        setAtomics(out.suggestedAtomics);
      } else if (out.suggestedAtomicsJson?.trim()) {
        try {
          const parsed = JSON.parse(out.suggestedAtomicsJson) as FlowAtomic[] | { atomics?: FlowAtomic[] };
          const arr = Array.isArray(parsed) ? parsed : Array.isArray(parsed?.atomics) ? parsed.atomics : [];
          if (arr.length > 0) setAtomics(arr);
          else message.warning("AI trả về suggestedAtomicsJson nhưng không parse được danh sách atomic.");
        } catch {
          message.warning("AI trả về suggestedAtomicsJson nhưng JSON không hợp lệ để đổ vào builder.");
        }
      }
      message.success("Đã tạo nháp atomic action bằng AI");
    } catch (e) {
      message.error(String(e));
    } finally {
      setAnalyzeBusy(false);
    }
  }, [analyzeAccountId, analyzeGoal, analyzeIntent, analyzeOutlineOnly, analyzeURL]);

  return (
    <div className="page" style={{ ...ui.page, maxWidth: "min(1320px, calc(100vw - 32px))", margin: "0 auto" }}>
      <Space wrap className="page-breadcrumb-links" style={{ marginBottom: 12 }}>
        <Link to="/flows/actions">
          <Button type="link">← Danh sách actions</Button>
        </Link>
        <Link to="/flows">
          <Button type="link">Danh sách flow</Button>
        </Link>
      </Space>

      <Card
        title={
          <div>
            <div style={{ fontWeight: 700 }}>Tạo action (kéo thả atomic)</div>
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              Lưu / sửa trực tiếp trên server (SQLite). Nháp vẫn đồng bộ trên trình duyệt. Copy JSON chỉ còn là tùy chọn khi cần dán ngoài.
            </Typography.Text>
          </div>
        }
        extra={
          <Space wrap>
            <Button type="primary" icon={<SaveOutlined />} loading={saving} onClick={() => void saveToServer()}>
              {savedId ? "Lưu thay đổi" : "Lưu lên server"}
            </Button>
            <Button icon={<PlusOutlined />} onClick={clearAll}>
              Action mới
            </Button>
            <Button icon={<CopyOutlined />} onClick={copyAtomicsOnly}>
              Copy chỉ atomics
            </Button>
            <Button icon={<CopyOutlined />} onClick={copyStepJson}>
              Copy JSON step
            </Button>
            <Button danger icon={<DeleteOutlined />} onClick={deleteFromServer} disabled={!savedId}>
              Xóa trên server
            </Button>
          </Space>
        }
      >
        <Card
          size="small"
          title="Tạo atomic action bằng AI (mở trình duyệt)"
          style={{ marginBottom: 16 }}
        >
          <Space direction="vertical" size="small" style={{ width: "100%" }}>
            <Select
              showSearch
              optionFilterProp="label"
              placeholder="Chọn account (profile/proxy)"
              value={analyzeAccountId || undefined}
              onChange={setAnalyzeAccountId}
              options={accounts.map((a) => ({ value: a.id, label: `${a.username ?? a.id} (${a.id})` }))}
            />
            <Input value={analyzeURL} onChange={(e) => setAnalyzeURL(e.target.value)} placeholder="URL cần mở để phân tích" />
            <Input.TextArea
              rows={2}
              value={analyzeGoal}
              onChange={(e) => setAnalyzeGoal(e.target.value)}
              placeholder="Mục tiêu probe (tuỳ chọn): ví dụ Mở panel comment và nhập text..."
            />
            <Input.TextArea
              rows={2}
              value={analyzeIntent}
              onChange={(e) => setAnalyzeIntent(e.target.value)}
              placeholder="Phạm vi action (tuỳ chọn): ví dụ chỉ follow user đầu tiên sau search"
            />
            <Checkbox checked={analyzeOutlineOnly} onChange={(e) => setAnalyzeOutlineOnly(e.target.checked)}>
              Chỉ phân tích snapshot DOM (không chạy probe)
            </Checkbox>
            <Button type="primary" loading={analyzeBusy} onClick={() => void runAnalyzeAtomic()}>
              Mở trình duyệt và tạo nháp action
            </Button>
            {analyzeResult ? (
              <Typography.Paragraph type="secondary" style={{ marginBottom: 0 }}>
                Trang: {analyzeResult.pageTitle || "—"} — {analyzeResult.pageUrl} — {analyzeResult.interactiveCount} control
                {analyzeResult.creationFlow ? ` — luồng: ${analyzeResult.creationFlow}` : ""}
              </Typography.Paragraph>
            ) : null}
          </Space>
        </Card>

        <Space direction="vertical" size="middle" style={{ width: "100%", marginBottom: 16 }}>
          <div>
            <Typography.Text strong style={{ display: "block", marginBottom: 6 }}>
              Mở action đã lưu
            </Typography.Text>
            <Select
              style={{ minWidth: 320, maxWidth: "100%" }}
              placeholder="Chọn để sửa… (hoặc để trống = bản nháp / mới)"
              allowClear
              loading={loadingSaved}
              value={savedId ?? undefined}
              options={savedList.map((s) => ({ value: s.id, label: `${s.name} (${s.id})` }))}
              onChange={(v) => {
                if (!v) {
                  setSearchParams({}, { replace: true });
                  return;
                }
                setSearchParams({ id: v }, { replace: true });
              }}
            />
          </div>
        </Space>

        {loadingSaved && savedId ? (
          <div style={{ marginBottom: 16 }}>
            <Spin /> <Typography.Text type="secondary"> Đang tải…</Typography.Text>
          </div>
        ) : null}

        <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 16 }}>
          Sau khi <b>Lưu lên server</b>, có thể mở flow → thêm step <code>playwright_atomics</code> → Import JSON (hoặc ghép tay). URL có <code>?id=…</code> để chia sẻ link sửa action.
        </Typography.Paragraph>

        <div style={{ marginBottom: 16 }}>
          <Typography.Text strong style={{ display: "block", marginBottom: 6 }}>
            Tên action
          </Typography.Text>
          <Input
            value={actionName}
            onChange={(e) => setActionName(e.target.value)}
            placeholder="vd: Follow user, Login email…"
            style={{ maxWidth: 480 }}
          />
        </div>

        <div style={{ marginBottom: 16 }}>
          <Typography.Text strong style={{ display: "block", marginBottom: 6 }}>
            Timeout step (giây)
          </Typography.Text>
          <Input
            type="number"
            min={1}
            value={timeoutSeconds}
            onChange={(e) => setTimeoutSeconds(Number(e.target.value) || 60)}
            style={{ maxWidth: 160 }}
          />
        </div>

        <StepParamsEditor params={stepParams} onChange={setStepParams} />

        <Typography.Text strong style={{ display: "block", marginBottom: 8 }}>
          Chuỗi atomic
        </Typography.Text>
        <Space wrap style={{ marginBottom: 12 }}>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            Preset trong <code>web/public/presets/</code> — flow (lấy <code>playwright_atomics</code>) hoặc file{" "}
            <code>.atomics.json</code>.
          </Typography.Text>
          <Button
            size="small"
            onClick={() => void loadPresetUrl(PRESET_FLOW_SEARCH_FOLLOW_URL, "preset TikTok search → follow")}
          >
            Tải: search → follow user đầu
          </Button>
          <Button
            size="small"
            onClick={() => void loadPresetUrl(PRESET_FLOW_OPEN_USER_AFTER_FOLLOW_URL, "preset mở User sau follow")}
          >
            Tải: mở User sau follow
          </Button>
          <Button
            size="small"
            onClick={() =>
              void loadPresetUrl(
                PRESET_ATOMICS_PROFILE_OPEN_FIRST_POST_URL,
                "preset profile — mở bài đăng đầu tiên"
              )
            }
          >
            Tải: profile — mở bài đầu
          </Button>
          <input
            ref={jsonFileInputRef}
            type="file"
            accept=".json,application/json"
            style={{ display: "none" }}
            onChange={(ev) => {
              const f = ev.target.files?.[0];
              if (!f) return;
              const reader = new FileReader();
              reader.onload = () => {
                applyAtomicsJsonText(String(reader.result ?? ""), f.name);
                ev.target.value = "";
              };
              reader.readAsText(f);
            }}
          />
          <Button size="small" icon={<UploadOutlined />} onClick={() => jsonFileInputRef.current?.click()}>
            Chọn file JSON
          </Button>
        </Space>
        <AtomicChainEditor atomics={atomics} onChange={setAtomics} />
      </Card>
    </div>
  );
}
