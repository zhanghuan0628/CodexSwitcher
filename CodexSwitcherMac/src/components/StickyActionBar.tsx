import type { ReactNode } from "react";

type StickyActionBarProps = {
  children: ReactNode;
};

export function StickyActionBar({ children }: StickyActionBarProps) {
  return <div className="sticky-action-bar">{children}</div>;
}

export type { StickyActionBarProps };
