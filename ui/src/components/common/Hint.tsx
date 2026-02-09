import React from "react";

type HintProps = {
  text: string;
};

export function Hint({ text }: HintProps) {
  return (
    <span
      title={text}
      aria-label={text}
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: 16,
        height: 16,
        borderRadius: "50%",
        border: "1px solid #c8d2dc",
        color: "#3b4c5d",
        fontSize: 11,
        lineHeight: "16px",
        cursor: "help",
        userSelect: "none"
      }}
    >
      ?
    </span>
  );
}
