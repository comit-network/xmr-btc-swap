import { Link, Typography } from '@material-ui/core';
import { ReactNode } from 'react';
import InfoBox from './InfoBox';

type TransactionInfoBoxProps = {
  title: string;
  txId: string;
  explorerUrl: string;
  additionalContent: ReactNode;
  loading: boolean;
  icon: JSX.Element;
};

export default function TransactionInfoBox({
  title,
  txId,
  explorerUrl,
  additionalContent,
  icon,
  loading,
}: TransactionInfoBoxProps) {
  return (
    <InfoBox
      title={title}
      mainContent={<Typography variant="h5">{txId}</Typography>}
      loading={loading}
      additionalContent={
        <>
          <Typography variant="subtitle2">{additionalContent}</Typography>
          <Typography variant="body1">
            <Link href={explorerUrl} target="_blank">
              View on explorer
            </Link>
          </Typography>
        </>
      }
      icon={icon}
    />
  );
}
