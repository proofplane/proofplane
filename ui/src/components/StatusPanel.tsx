import type { ReactNode } from "react";

type StatusPanelProps = {
  title: string;
  children: ReactNode;
  tone?: "default" | "preview";
};

export function StatusPanel({
  title,
  children,
  tone = "default",
}: StatusPanelProps) {
  return (
    <section className={`status-panel status-panel-${tone}`}>
      <h2>{title}</h2>
      <div>{children}</div>
    </section>
  );
}
