import "./App.css";
import Stepper from "./components/ui/stepper";
import Experience from "./Experience";

export default function App() {
  return (
    <div className="h-screen min-h-screen w-screen">
      <Stepper
        titles={["ESC Telemetry", "Gyroscope", "Accelerometer", "Magnetometer"]}
        activeStep={1}
      />
      <Experience />
    </div>
  );
}
