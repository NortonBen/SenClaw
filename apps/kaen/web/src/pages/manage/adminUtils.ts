import api from '@/lib/api';

/** Save any JSON payload to disk as a download. */
export function downloadJson(data: unknown, filename: string) {
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
}

/** Read a user-picked file and parse it as JSON. */
export function readJsonFile(file: File): Promise<unknown> {
    return new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => {
            try {
                resolve(JSON.parse(String(reader.result)));
            } catch {
                reject(new Error('File không phải JSON hợp lệ'));
            }
        };
        reader.onerror = () => reject(new Error('Không đọc được file'));
        reader.readAsText(file);
    });
}

export function stamp() {
    const d = new Date();
    const p = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`;
}

/** Axios errors carry the message under different keys depending on the layer. */
export function errText(e: unknown, fallback = 'Có lỗi xảy ra'): string {
    const r = (e as { response?: { data?: { error?: string; message?: string } } })?.response?.data;
    return r?.error || r?.message || (e as Error)?.message || fallback;
}

export const LEVELS = ['A1', 'A2', 'B1', 'B1-B2', 'B2', 'C1', 'OTHER'];

export { api };
