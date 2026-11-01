import { useState } from "react";

interface DepthPickerProps {
  /** null means every level. */
  value: number | null;
  presets: readonly (number | null)[];
  /** What the custom box holds while a preset is chosen. */
  custom: number;
  max: number;
  title?: string;
  onChange: (depth: number | null) => void;
  onCustom: (depth: number) => void;
}

export function DepthPicker({
  value,
  presets,
  custom,
  max,
  title,
  onChange,
  onCustom,
}: DepthPickerProps) {
  const isCustom = !presets.includes(value);
  const firstCustom = Math.max(...presets.map((d) => d ?? 0)) + 1;
  const [text, setText] = useState(String(custom));

  const pick = (n: number) => {
    const depth = Math.min(n, max);
    onCustom(depth);
    onChange(depth);
  };

  return (
    <div
      className="mt-1.5 flex rounded-md border border-edge p-0.5"
      title={title}
    >
      {presets.map((d) => (
        <button
          key={d ?? "all"}
          onClick={() => onChange(d)}
          className={`h-6 flex-1 rounded text-[12px] ${
            value === d ? "bg-raised text-ink" : "text-ink-4 hover:text-ink-2"
          }`}
        >
          {d ?? "All"}
        </button>
      ))}
      <input
        value={text}
        onFocus={() => pick(custom)}
        onChange={(e) => {
          const digits = e.target.value
            .replace(/\D/g, "")
            .slice(0, String(max).length);
          setText(digits);
          if (Number(digits) >= 1) pick(Number(digits));
        }}
        onBlur={() => setText(String(custom))}
        placeholder={`${firstCustom}+`}
        title={`Any depth up to ${max}`}
        className={`h-6 w-0 min-w-0 flex-1 rounded text-center text-[12px] outline-none placeholder:text-ink-5 ${
          isCustom
            ? "bg-raised text-ink"
            : "bg-transparent text-ink-4 hover:text-ink-2"
        }`}
      />
    </div>
  );
}
