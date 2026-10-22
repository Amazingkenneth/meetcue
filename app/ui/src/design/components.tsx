import * as RadixSwitch from "@radix-ui/react-switch";
import { useId, type ReactNode } from "react";

export function Field({
  label,
  hint,
  children,
  htmlFor,
}: {
  label: ReactNode;
  hint?: ReactNode;
  children: ReactNode;
  htmlFor?: string;
}) {
  return (
    <div className="field">
      {htmlFor ? <label htmlFor={htmlFor}>{label}</label> : <span className="label">{label}</span>}
      <div className="control">{children}</div>
      {hint && <p className="hint">{hint}</p>}
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  label,
  description,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: ReactNode;
  description?: ReactNode;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="check-row">
      <RadixSwitch.Root
        id={id}
        className="switch"
        checked={checked}
        onCheckedChange={onChange}
        disabled={disabled}
        aria-describedby={description ? `${id}-d` : undefined}
      >
        <RadixSwitch.Thumb className="switch-thumb" />
      </RadixSwitch.Root>
      <label className="text" htmlFor={id}>
        <strong>{label}</strong>
        {description && <span id={`${id}-d`}>{description}</span>}
      </label>
    </div>
  );
}

export function Section({ title, lead, children }: { title?: ReactNode; lead?: ReactNode; children: ReactNode }) {
  return (
    <section className="section">
      {title && <h2>{title}</h2>}
      {lead && <p className="lead">{lead}</p>}
      {children}
    </section>
  );
}

export function Badge({ tone, children }: { tone?: "ok" | "warn" | "err"; children: ReactNode }) {
  return <span className={`badge${tone ? ` badge-${tone}` : ""}`}>{children}</span>;
}
