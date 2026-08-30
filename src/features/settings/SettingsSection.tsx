import { useCallback, type ReactNode } from "react";
import type { SettingsSectionId } from "./settingsSections";

type SettingsSectionProps = {
  children: ReactNode;
  description: string;
  id: SettingsSectionId;
  registerSection: (id: SettingsSectionId, element: HTMLElement | null) => void;
  title: string;
};

export function SettingsSection({
  children,
  description,
  id,
  registerSection,
  title,
}: SettingsSectionProps) {
  const handleRef = useCallback(
    (node: HTMLElement | null) => {
      registerSection(id, node);
    },
    [id, registerSection],
  );

  return (
    <section className="scroll-mt-16 space-y-1.5" id={id} ref={handleRef}>
      <header className="space-y-0.5">
        <h2 className="text-base font-semibold text-pg-fg-default">{title}</h2>
        <p className="text-xs leading-relaxed text-pg-fg-muted">{description}</p>
      </header>
      <div className="space-y-2.5">{children}</div>
    </section>
  );
}
