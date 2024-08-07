import { ReactNode } from 'react';
import { Box, Typography } from '@material-ui/core';
import FileCopyOutlinedIcon from '@material-ui/icons/FileCopyOutlined';
import InfoBox from './InfoBox';
import ClipboardIconButton from './ClipbiardIconButton';
import BitcoinQrCode from './BitcoinQrCode';

type Props = {
  title: string;
  address: string;
  additionalContent: ReactNode;
  icon: ReactNode;
};

export default function DepositAddressInfoBox({
  title,
  address,
  additionalContent,
  icon,
}: Props) {
  return (
    <InfoBox
      title={title}
      mainContent={<Typography variant="h5">{address}</Typography>}
      additionalContent={
        <Box>
          <Box>
            <ClipboardIconButton
              text={address}
              endIcon={<FileCopyOutlinedIcon />}
              color="primary"
              variant="contained"
              size="medium"
            />
            <Box
              style={{
                display: 'flex',
                flexDirection: 'row',
                gap: '0.5rem',
                alignItems: 'center',
              }}
            >
              <Box>{additionalContent}</Box>
              <BitcoinQrCode address={address} />
            </Box>
          </Box>
        </Box>
      }
      icon={icon}
      loading={false}
    />
  );
}
