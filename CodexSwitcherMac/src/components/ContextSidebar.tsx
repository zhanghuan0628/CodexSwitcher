import type { ReactNode } from "react";
import type { SidebarAction, SidebarSectionData } from "../shell/layout";

type ContextSidebarProps = {
  identity: ReactNode;
  actions?: SidebarAction[];
  sections?: SidebarSectionData[];
  footer?: ReactNode;
};

const toneClassMap: Record<NonNullable<SidebarAction["tone"]>, string> = {
  primary: "btn-primary",
  secondary: "btn-secondary",
  ghost: "btn-ghost",
};

const itemToneClassMap: Record<NonNullable<SidebarSectionData["items"][number]["tone"]>, string> = {
  default: "context-sidebar__item-value--default",
  success: "context-sidebar__item-value--success",
  warning: "context-sidebar__item-value--warning",
  danger: "context-sidebar__item-value--danger",
  muted: "context-sidebar__item-value--muted",
};

export function ContextSidebar({ identity, actions, sections, footer }: ContextSidebarProps) {
  return (
    <div className="context-sidebar card">
      <div className="context-sidebar__identity">{identity}</div>

      {actions?.length ? (
        <div className="context-sidebar__actions">
          {actions.map((action) => (
            <button
              key={action.key}
              className={`btn ${toneClassMap[action.tone ?? "secondary"]}`}
              type="button"
              disabled={action.disabled}
              onClick={action.onClick}
            >
              {action.label}
            </button>
          ))}
        </div>
      ) : null}

      {sections?.length ? (
        <div className="context-sidebar__sections">
          {sections.map((section) => (
            <section className="context-sidebar__section" key={section.key}>
              <p className="eyebrow">{section.title}</p>
              <div className="context-sidebar__items">
                {section.items.map((item) => (
                  <div className="context-sidebar__item" key={`${section.key}-${item.label}`}>
                    <span className="context-sidebar__item-label">{item.label}</span>
                    <span className={`context-sidebar__item-value ${itemToneClassMap[item.tone ?? "default"]}`}>
                      {item.value ?? "—"}
                    </span>
                  </div>
                ))}
              </div>
            </section>
          ))}
        </div>
      ) : null}

      {footer ? <div className="context-sidebar__footer">{footer}</div> : null}
    </div>
  );
}
