import React from "react";

export function Widget() {
    function helper(name: string) {
        return name.toUpperCase();
    }

    const helper2 = function (value: number) {
        return value + 1;
    };

    return <div>{helper("world")}</div>;
}
