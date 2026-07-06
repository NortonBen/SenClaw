import { useEffect, useRef, useState } from 'react';

export interface ModalSpec {
  title: string;
  placeholder?: string;
  initial?: string;
  okLabel?: string;
  onSubmit: (value: string) => void;
}

/** A small in-app text-input modal, replacing window.prompt(). */
export function InputModal({ spec, onClose }: { spec: ModalSpec | null; onClose: () => void }) {
  const [value, setValue] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (spec) {
      setValue(spec.initial ?? '');
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [spec]);

  if (!spec) return null;

  function submit() {
    const v = value.trim();
    if (!v) return;
    spec!.onSubmit(v);
    onClose();
  }

  return (
    <div className="modal-overlay" onMouseDown={onClose}>
      <div className="modal-card" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-title">{spec.title}</div>
        <input
          ref={inputRef}
          value={value}
          placeholder={spec.placeholder}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') { e.preventDefault(); submit(); }
            if (e.key === 'Escape') { e.preventDefault(); onClose(); }
          }}
        />
        <div className="modal-actions">
          <button className="btn ghost" onClick={onClose}>Huỷ</button>
          <button className="btn" disabled={!value.trim()} onClick={submit}>{spec.okLabel ?? 'OK'}</button>
        </div>
      </div>
    </div>
  );
}
