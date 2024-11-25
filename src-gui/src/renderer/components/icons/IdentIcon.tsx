import React, { useEffect, useRef } from 'react';
import * as jdenticon from 'jdenticon';

interface IdentIconProps {
    value: string;
    size?: number | string;
    className?: string;
}

function IdentIcon({ value, size = 40, className = '' }: IdentIconProps) {
    const iconRef = useRef<SVGSVGElement>(null);

    useEffect(() => {
        if (iconRef.current) {
            jdenticon.update(iconRef.current, value);
        }
    }, [value]);

    return (
        <svg
            ref={iconRef}
            width={size}
            height={size}
            className={className}
            data-jdenticon-value={value} />
    );
}

export default IdentIcon;
