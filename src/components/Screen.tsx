import type { ReactNode } from "react";

export function Screen({
  header,
  footer,
  children,
  className = "",
}: {
  header?: ReactNode;
  footer?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={`screen${className ? ` ${className}` : ""}`}>
      {header && <header className="screen__header">{header}</header>}
      <div className="screen__body">{children}</div>
      {footer && <footer className="screen__footer">{footer}</footer>}
    </div>
  );
}
