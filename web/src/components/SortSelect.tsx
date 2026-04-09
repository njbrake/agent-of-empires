export type SortOrder =
  | "created-desc"
  | "created-asc"
  | "accessed-desc"
  | "accessed-asc"
  | "title-asc"
  | "title-desc";

const SORT_LABELS: Record<SortOrder, string> = {
  "created-desc": "Newest",
  "created-asc": "Oldest",
  "accessed-desc": "Recent",
  "accessed-asc": "Least recent",
  "title-asc": "A-Z",
  "title-desc": "Z-A",
};

interface Props {
  value: SortOrder;
  onChange: (value: SortOrder) => void;
}

export function SortSelect({ value, onChange }: Props) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value as SortOrder)}
      className="bg-transparent font-mono text-[10px] text-slate-600 hover:text-slate-400 cursor-pointer border-none focus:outline-none appearance-none"
      title="Sort sessions"
    >
      {Object.entries(SORT_LABELS).map(([key, label]) => (
        <option key={key} value={key} className="bg-surface-800 text-slate-300">
          {label}
        </option>
      ))}
    </select>
  );
}
