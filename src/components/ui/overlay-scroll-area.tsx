import type { ComponentProps } from "react";
import { OverlayScrollbarsComponent } from "overlayscrollbars-react";

type OverlayScrollAreaProps = Omit<ComponentProps<"div">, "ref"> & { element?: "div" | "main" };

/** Native scrolling with an overlay track that does not take layout space. */
export function OverlayScrollArea({ element = "div", children, ...props }: OverlayScrollAreaProps) {
  return (
    <OverlayScrollbarsComponent
      element={element}
      options={{
        scrollbars: { theme: "os-theme-wb", autoHide: "leave", autoHideDelay: 800, clickScroll: "instant" },
      }}
      {...props}
    >
      {children}
    </OverlayScrollbarsComponent>
  );
}
