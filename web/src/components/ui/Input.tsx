import { clsx } from "clsx";

interface Props extends React.InputHTMLAttributes<HTMLInputElement> {
  mono?: boolean;
}

/** Shared text input styled per DESIGN-WEB.md */
export function Input({ className, mono, ...props }: Props) {
  return (
    <input
      {...props}
      className={clsx(
        "w-full bg-surface-900 border border-surface-700 rounded px-3 py-1.5 text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none",
        mono ? "font-mono" : "font-body",
        className,
      )}
    />
  );
}
