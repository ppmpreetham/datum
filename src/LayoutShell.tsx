import { Outlet } from "@tanstack/react-router";
import { JSX } from "react";

export function LayoutShell(): JSX.Element {
  return (
    <div className="min-h-screen">
      <Outlet />
    </div>
  );
}
