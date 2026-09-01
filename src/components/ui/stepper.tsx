import React from "react";
import clsx from "clsx";

interface StepperProps {
  titles: string[];
  activeStep: number;
}

export default function Stepper({ titles, activeStep }: StepperProps) {
  const totalSteps = titles.length;

  return (
    <div className="fixed top-0 left-0 right-0 z-50 w-full bg-transparent p-6 pb-12">
      <div className="flex items-center w-full max-w-2xl mx-auto">
        {titles.map((title, index) => {
          const isActive = index === activeStep;
          const isCurrentPast = index <= activeStep;
          const isFuture = index > activeStep;

          return (
            <React.Fragment key={index}>
              <div className="flex items-center relative">
                <div
                  className={clsx(
                    "relative w-8 h-8 rounded-full flex items-center justify-center font-medium text-sm transition-colors duration-200 bg-gray-100 text-black",
                    isActive &&
                      "after:absolute after:-inset-1.5 after:rounded-full after:border-2 after:border-white",
                    isFuture && "bg-transparent border-2 border-gray-300 text-gray-400",
                  )}
                ></div>

                {isCurrentPast && (
                  <span
                    className={clsx(
                      "absolute top-12 left-1/2 -translate-x-1/2 text-xs font-medium text-center transition-colors duration-200",
                      "text-white",
                    )}
                  >
                    {title}
                  </span>
                )}
              </div>

              {index < totalSteps - 1 && (
                <div
                  className={clsx("flex-1 h-0.5 mx-4 transition-colors duration-300 bg-gray-200")}
                />
              )}
            </React.Fragment>
          );
        })}
      </div>
    </div>
  );
}
