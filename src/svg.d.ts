// Override vite/client's default *.svg type (string URL) because
// vite-plugin-svgr converts all SVG imports into React components.
declare module "*.svg" {
  import * as React from "react";

  const ReactComponent: React.FunctionComponent<
    React.ComponentProps<"svg"> & { title?: string; titleId?: string }
  >;

  export default ReactComponent;
}
