export async function api<T>(
  path: string,
  method: "GET" | "POST" | "PUT" | "DELETE" = "GET",
  body?: unknown
): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const rawText = await res.text();
  let parsed: unknown = null;
  if (rawText.trim()) {
    try {
      parsed = JSON.parse(rawText);
    } catch {
      parsed = rawText;
    }
  }

  const envelope =
    parsed && typeof parsed === "object" && "success" in parsed
      ? (parsed as { success?: boolean; msg?: string; data?: unknown })
      : null;

  if (!res.ok) {
    if (envelope) throw new Error(envelope.msg || "Request failed");
    throw new Error(typeof parsed === "string" ? parsed : "Request failed");
  }

  if (envelope) {
    if (envelope.success === false) {
      throw new Error(envelope.msg || "Request failed");
    }
    return envelope.data as T;
  }
  return parsed as T;
}
