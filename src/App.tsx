import "./App.css";
import { RouterProvider } from "@tanstack/react-router";
import { router } from "./main";

export default function App() {
  return (
    <div className="h-screen min-h-screen w-screen">
      <RouterProvider router={router} />
    </div>
  );
}
