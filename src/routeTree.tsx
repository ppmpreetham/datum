import { createRootRoute, createRoute } from "@tanstack/react-router";
import { GyroSender } from "./phone";
import { GyroReceiverPage } from "./utils/ws_reciever";
import { Outlet } from "@tanstack/react-router";
import App from "./App";

const rootRoute = createRootRoute({
  component: () => <Outlet />,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: App,
});

const senderRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sender",
  component: GyroSender,
});

const receiverRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/receiver",
  component: GyroReceiverPage,
});

export const routeTree = rootRoute.addChildren([senderRoute, receiverRoute, indexRoute]);
