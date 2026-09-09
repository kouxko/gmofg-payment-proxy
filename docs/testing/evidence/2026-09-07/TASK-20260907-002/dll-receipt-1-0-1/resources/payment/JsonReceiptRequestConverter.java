/*
 *  = COPYRIGHT
 *           PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION
 *    This software is supplied under the terms of a license agreement or nondisclosure
 *    agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or
 *    disclosed except in accordance with the terms in that agreement.
 *    Copyright (C) 2019-? Computer Technology(Shenzhen) CO., LTD All rights reserved.
 */
package jp.gmofg.payment.pay.trans.receipt;

import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Color;
import android.text.TextUtils;
import android.util.Base64;

import androidx.annotation.NonNull;

import com.pax.gl.pack.ITlv;

import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;

import jp.gmofg.common.util.ByteUtil;
import jp.gmofg.common.util.PLog;
import jp.gmofg.common.util.StringUtil;
import jp.gmofg.core.PaxGLPackerProvider;
import jp.gmofg.emv.entity.IBrand;
import jp.gmofg.emv.entity.TagsTable;
import jp.gmofg.msg.receipt.DenpyoInfo;
import jp.gmofg.msg.receipt.GyomuInfo;
import jp.gmofg.msg.receipt.IcInfo;
import jp.gmofg.msg.receipt.JsonReceipt;
import jp.gmofg.msg.trans.payment.PaymentMethod;
import jp.gmofg.msg.trans.payment.adapter.PaymentMethodAdapter;
import jp.gmofg.payment.R;
import jp.gmofg.payment.data.entity.CardCompany;
import jp.gmofg.payment.data.entity.TransReceipt;
import jp.gmofg.payment.data.local.Repository;
import jp.gmofg.payment.domain.usecase.LauncherAppSettingsUseCase;
import jp.gmofg.payment.pay.trans.model.TransType;
import jp.gmofg.payment.pay.utils.AndroidUtil;
import jp.gmofg.payment.settings.AppPreference;

/**
 * The type Json receipt request converter.
 */
public class JsonReceiptRequestConverter {

    /**
     * Convert json receipt . request.
     *
     * @param transReceipt the trans data
     * @return the json receipt . request
     */
    public static JsonReceipt.Request convert(TransReceipt transReceipt) {
        if (transReceipt.getIsUnionPay()) {
            return convertUnionPay(transReceipt);
        }
        return convertCredit(transReceipt);
    }

    /**
     * convert TransReceipt to JsonReceipt request
     *
     * @param transReceipt receipt info
     * @return request
     */
    private static JsonReceipt.Request convertCredit(TransReceipt transReceipt) {
        JsonReceipt.Request request = new JsonReceipt.Request();
        request.setTransactionType("U010");
        GyomuInfo gyomuInfo = new GyomuInfo();
        DenpyoInfo denpyoInfo = new DenpyoInfo();
        denpyoInfo.setReceiptTitle(AndroidUtil.getString(R.string.receipt_title_credit));
        denpyoInfo.setMerchantName(
                LauncherAppSettingsUseCase.getINSTANCE().getMerchantName() + '\n' +
                        LauncherAppSettingsUseCase.getINSTANCE().getMerchantTel());
        if (transReceipt.getPanInputMode().isIcc()) {
            denpyoInfo.setKessaiSyubeu("02");
            denpyoInfo.setCardInfoResource("IC");
            gyomuInfo.setIcInfo(getIcInfoFromEmvTags(transReceipt.getEmvTags(), false, true));
        }
        if (transReceipt.getPanInputMode().isPicc()) {
            denpyoInfo.setKessaiSyubeu("03");
            denpyoInfo.setCardInfoResource("IC");
            gyomuInfo.setIcInfo(getIcInfoFromEmvTags(transReceipt.getEmvTags(), false, false));
        }
        if (transReceipt.getPanInputMode().isMag() || transReceipt.getPanInputMode().isFallback()) {
            denpyoInfo.setKessaiSyubeu("01");
            denpyoInfo.setCardInfoResource("MS");
        }
        if (transReceipt.getPanInputMode().isManual()) {
            denpyoInfo.setKessaiSyubeu("01");
            denpyoInfo.setCardInfoResource("MN");
        }

        String pan = transReceipt.getPan();
        if (pan != null && transReceipt.getVenusCardCompanyCode().isEmpty()) {
            CardCompany companyInfoByPan = Repository.getInstance()
                    .findCardCompanyInfoByPan(pan.replace('*', '0'));
            String acqCode = "";
            if (companyInfoByPan != null) {
                acqCode = companyInfoByPan.getAcquirerCode();
            }
            denpyoInfo.setCardCompanyCode(StringUtil.rightPadSpace(acqCode, 11));
        } else {
            denpyoInfo.setCardCompanyCode(transReceipt.getVenusCardCompanyCode());
        }
        denpyoInfo.setTerminalSeqNo(transReceipt.getTransNo());
        PaymentMethod.Type type = transReceipt.getPaymentMethod().getType();
        TransType transType = transReceipt.getTransType();

        if (transType == TransType.SALE) {
            denpyoInfo.setServiceCode("1");
            denpyoInfo.setShiharaiKubun(type.typeCode());
            denpyoInfo.setToriatukaiKubun(type.voidPaymentCode());
        }
        if (transType == TransType.PRE_AUTH_COMP) {
            denpyoInfo.setServiceCode("3");
            denpyoInfo.setShiharaiKubun(type.typeCode());
            denpyoInfo.setToriatukaiKubun(type.voidPaymentCode());
        }
        if (transType == TransType.PRE_AUTH) {
            denpyoInfo.setServiceCode("4");
            denpyoInfo.setToriatukaiKubun(type.voidPaymentCode());
        }
        denpyoInfo.setShiharaiKubun(transReceipt.getPaymentMethod().getPaymentTypeCode());
        if (transReceipt.getTransType() == TransType.VOID) {
            denpyoInfo.setCancelTerminalSeqNo(transReceipt.getOrigTransNo());
            TransType origTransType = transReceipt.getOriginalTransType();
            if (origTransType == TransType.SALE) {
                denpyoInfo.setServiceCode("2");
                denpyoInfo.setToriatukaiKubun(type.voidPaymentCode());
            }
            if (origTransType == TransType.PRE_AUTH_COMP) {
                denpyoInfo.setServiceCode("5");
                denpyoInfo.setToriatukaiKubun(type.voidPaymentCode());
            }
            if (origTransType == TransType.PRE_AUTH) {
                denpyoInfo.setServiceCode("6");
                denpyoInfo.setToriatukaiKubun("200");
            }
            if (transReceipt.getTransDate()
                    .substring(0, 8)
                    .equals(transReceipt.getOriginalTransDate().substring(0, 8))) {
                denpyoInfo.setTorikeshiKubun("1");
            } else {
                denpyoInfo.setTorikeshiKubun("2");
            }
        }
        denpyoInfo.setRiyoubi(transReceipt.getTransDate());
        denpyoInfo.setMemberShipNo(pan);
        denpyoInfo.setKanaName(transReceipt.getHolderName());
        String yyMM = transReceipt.getExpDate().replace("/", "");
        denpyoInfo.setYuKoKigen(yyMM.substring(2) + yyMM.substring(0, 2));
        denpyoInfo.setTerminalId(AppPreference.getCreditTerminalId());
        denpyoInfo.setProcessSeqNo(transReceipt.getProcessSequenceNo());

        PaymentMethodAdapter adapter = PaymentMethodAdapter.bind(transReceipt.getPaymentMethod());

        try {
            if (transReceipt.getTransType() == TransType.PRE_AUTH) {
                throw new ClassCastException();
            }
            int installmentTimes = adapter.getInstallmentTimes();
            if (installmentTimes == 0) {
                throw new ClassCastException();
            }
            denpyoInfo.setShiharaiKaisu(installmentTimes);
        } catch (ClassCastException e) {
            //do nothing
        }
        if (StringUtil.isEmpty(transReceipt.getProductCode())) {
            denpyoInfo.setItemCode(AppPreference.getDefaultProductCode());
        } else {
            denpyoInfo.setItemCode(transReceipt.getProductCode());
        }
        denpyoInfo.setApprovalNo(transReceipt.getApprovalCode());
        denpyoInfo.setAmountInfo(transReceipt.getAmount(), transReceipt.getTax());
        setSignInfoFromTransReceipt(denpyoInfo, transReceipt);
        gyomuInfo.setDenpyoInfo(denpyoInfo);
        PLog.d(gyomuInfo.toJsonString());
        request.setEncryptionInfo(AppPreference.getCreditTerminalId(),
                AppPreference.getConfirmCode(), AppPreference.getPassword(), gyomuInfo);
        return request;
    }

    /**
     * convert TransReceipt to JsonReceipt request
     *
     * @param transReceipt receipt info
     * @return request
     */
    private static JsonReceipt.Request convertUnionPay(TransReceipt transReceipt) {
        JsonReceipt.Request request = new JsonReceipt.Request();
        request.setTransactionType("U011");
        GyomuInfo gyomuInfo = new GyomuInfo();
        DenpyoInfo denpyoInfo = new DenpyoInfo();
        denpyoInfo.setKessaiSyubeu("04");
        denpyoInfo.setReceiptTitle(AndroidUtil.getString(R.string.receipt_title_union));
        denpyoInfo.setMerchantName(
                LauncherAppSettingsUseCase.getINSTANCE().getMerchantName() + '\n' +
                        LauncherAppSettingsUseCase.getINSTANCE().getMerchantTel());
        if (transReceipt.getPanInputMode().isIcc()) {
            denpyoInfo.setCardInfoResource("IC");
            gyomuInfo.setIcInfo(getIcInfoFromEmvTags(transReceipt.getEmvTags(), true, true));
        }
        if (transReceipt.getPanInputMode().isPicc()) {
            denpyoInfo.setCardInfoResource("IC");
            gyomuInfo.setIcInfo(getIcInfoFromEmvTags(transReceipt.getEmvTags(), true, false));
        }
        if (transReceipt.getPanInputMode().isMag() || transReceipt.getPanInputMode().isFallback()) {
            denpyoInfo.setCardInfoResource("MS");
            gyomuInfo.setIcInfo(new IcInfo());
        }
        if (transReceipt.getPanInputMode().isManual()) {
            denpyoInfo.setCardInfoResource("MN");
            gyomuInfo.setIcInfo(new IcInfo());
        }
        String acqId = Repository.getInstance().getUnionPayInfo().getAcquirerId();
        denpyoInfo.setCardCompanyCode(StringUtil.rightPadSpace(acqId, 11));
        denpyoInfo.setTerminalSeqNo(transReceipt.getTransNo());
        if (transReceipt.getTransType() == TransType.UNION_VOID) {
            denpyoInfo.setCancelTerminalSeqNo(transReceipt.getOrigTransNo());
            if (transReceipt.getTransDate()
                    .substring(0, 8)
                    .equals(transReceipt.getOriginalTransDate().substring(0, 8))) {
                denpyoInfo.setTorikeshiKubun("1");
            } else {
                denpyoInfo.setTorikeshiKubun("2");
            }
        }
        denpyoInfo.setRiyoubi(transReceipt.getTransDate());
        denpyoInfo.setMemberShipNo(transReceipt.getPan());
        denpyoInfo.setKanaName(transReceipt.getHolderName());
        String yyMM = transReceipt.getExpDate().replace("/", "");
        denpyoInfo.setYuKoKigen(yyMM.substring(2) + yyMM.substring(0, 2));
        denpyoInfo.setTerminalId(AppPreference.getUnionPayTerminalId());
        denpyoInfo.setProcessSeqNo(transReceipt.getProcessSequenceNo());
        denpyoInfo.setItemCode(Repository.getInstance().getUnionPayInfo().getRetailStoreType());
        TransType transType = transReceipt.getTransType();
        if (transType == TransType.UNION_SALE) {
            denpyoInfo.setServiceCode("1");
        }
        if (transType == TransType.UNION_PRE_AUTH_COMP) {
            denpyoInfo.setServiceCode("3");
        }
        if (transType == TransType.UNION_PRE_AUTH) {
            denpyoInfo.setServiceCode("4");
        }
        if (transType == TransType.UNION_VOID) {
            TransType origTransType = transReceipt.getOriginalTransType();
            if (origTransType == TransType.UNION_SALE) {
                denpyoInfo.setServiceCode("2");
            }
            if (origTransType == TransType.UNION_PRE_AUTH_COMP) {
                denpyoInfo.setServiceCode("5");
            }
            if (origTransType == TransType.UNION_PRE_AUTH) {
                denpyoInfo.setServiceCode("6");
            }
            if (transReceipt.getTransDate()
                    .substring(0, 8)
                    .equals(transReceipt.getOriginalTransDate().substring(0, 8))) {
                denpyoInfo.setTorikeshiKubun("1");
            } else {
                denpyoInfo.setTorikeshiKubun("2");
            }
        }
        denpyoInfo.clearShiharaiKaisu();
        denpyoInfo.setItemCode(
                "000" + Repository.getInstance().getUnionPayInfo().getRetailStoreType());
        denpyoInfo.setApprovalNo(transReceipt.getApprovalCode());
        denpyoInfo.setAmountInfo(transReceipt.getAmount(), transReceipt.getTax());
        setSignInfoFromTransReceipt(denpyoInfo, transReceipt);
        gyomuInfo.setDenpyoInfo(denpyoInfo);
        //union pay info
        String bankName = Repository.getInstance().getUnionPayInfo().getBankName();
        gyomuInfo.setGinrenInfo(bankName, transReceipt.getOriginalProcNo(),
                transReceipt.getOriginalDateTime());
        PLog.d(gyomuInfo.toJsonString());

        //set enc info
        request.setEncryptionInfo(AppPreference.getUnionPayTerminalId(),
                AppPreference.getConfirmCode(), AppPreference.getPassword(), gyomuInfo);
        return request;
    }

    /**
     * get ic info form emv tags
     *
     * @param emvTags    emv tags
     * @param isUnionPay union pay
     * @param isContact  is contact chip
     * @return ic info
     */
    private static IcInfo getIcInfoFromEmvTags(byte[] emvTags, boolean isUnionPay,
                                               boolean isContact) {
        if (emvTags == null || emvTags.length == 0) {
            return null;
        }
        IcInfo icInfo = new IcInfo();
        try {
            final ITlv.ITlvDataObjList tlvDataObjList =
                    PaxGLPackerProvider.getTlv().unpack(emvTags);
            final byte[] aid = tlvDataObjList.getValueByTag(TagsTable.AID);
            final IBrand brand =
                    Repository.getInstance().findBrandIdByAID(isUnionPay, isContact, aid);
            switch (brand.getBrandId()) {
                case "31":
                case "35":
                    icInfo.setBrandName("JCB       ");
                    break;
                case "32":
                case "37":
                    icInfo.setBrandName("AMEX      ");
                    break;
                case "33":
                case "36":
                    icInfo.setBrandName("Diners    ");
                    break;
                case "40":
                case "41":
                    icInfo.setBrandName("VISA     ");
                    break;
                case "50":
                case "51":
                    icInfo.setBrandName("MasterCard");
                    break;
                default:
                    icInfo.setBrandName("          ");
                    break;
            }
            String aidStr = ByteUtil.byteArrayToHexString(aid);
            icInfo.setAid(StringUtil.rightPadSpace(aidStr, 32));
            String brandIdentifier = StringUtil.EMPTY;
            String appPreferredName;
            try {
                appPreferredName =
                        new String(tlvDataObjList.getValueByTag(TagsTable.APP_PREFERRED_NAME),
                                StandardCharsets.US_ASCII);
            } catch (Exception e) {
                appPreferredName = "";
            }
            String appLabel;
            try {
                appLabel = new String(tlvDataObjList.getValueByTag(TagsTable.APP_LABEL),
                        StandardCharsets.US_ASCII);
            } catch (Exception e) {
                appLabel = "";
            }
            if (!TextUtils.isEmpty(appPreferredName)) {
                brandIdentifier = appPreferredName;
            } else if (!TextUtils.isEmpty(appLabel)) {
                brandIdentifier = appLabel;
            } else if (aidStr.length() > 10 && aidStr.startsWith("A000000152")) {
                brandIdentifier = "Diners Club/Discover";
            }
            icInfo.setApplicationLabel(brandIdentifier);
            try {
                String hexAtc = ByteUtil.byteArrayToHexString(
                        tlvDataObjList.getValueByTag(TagsTable.APP_TRANS_COUNTER));
                icInfo.setAtc(StringUtil.rightPadSpace(hexAtc, 5));
            } catch (Exception e) {
                //do nothing
                icInfo.setAtc("0000 ");
            }
            try {
                icInfo.setCardSeqNo(ByteUtil.byteArrayToHexString(
                        tlvDataObjList.getValueByTag(TagsTable.PAN_SEQ_NO)));
            } catch (Exception e) {
                icInfo.setCardSeqNo("00");
            }
        } catch (Exception e) {
            PLog.d(e);
        }
        return icInfo;
    }

    /**
     * set signInfo form transReceipt
     *
     * @param denpyoInfo   json receipt body
     * @param transReceipt receipt info
     */
    private static void setSignInfoFromTransReceipt(DenpyoInfo denpyoInfo,
                                                    TransReceipt transReceipt) {
        byte[] signData = transReceipt.getSignData();
        if (signData != null) {
            if (signData.length > 0) {
                denpyoInfo.setSignatureInfo('0', signatureToBase64(signData), null);
            } else {
                denpyoInfo.setSignatureInfo('4', null, null);
            }
        } else if (transReceipt.getIsPinVerified()) {
            denpyoInfo.setSignatureInfo('1', null,
                    AndroidUtil.getString(R.string.receipt_default_pin_verified));
        } else {
            denpyoInfo.setSignatureInfo('1', null,
                    AndroidUtil.getString(R.string.receipt_default_no_sign));
        }
    }

    private static String signatureToBase64(@NonNull byte[] signature) {
        Bitmap signBitmap = BitmapFactory.decodeByteArray(signature, 0, signature.length);
        Bitmap resultBitmap = Bitmap.createBitmap(230, 95, Bitmap.Config.RGB_565);
        resultBitmap.eraseColor(Color.WHITE);
        Canvas canvas = new Canvas(resultBitmap);
        canvas.drawBitmap(signBitmap, (float) (canvas.getWidth() - signBitmap.getWidth()) / 2,
                (float) (canvas.getHeight() - signBitmap.getHeight()) / 2, null);
        ByteArrayOutputStream byteArrayOutputStream = new ByteArrayOutputStream();
        resultBitmap.compress(Bitmap.CompressFormat.PNG, 100, byteArrayOutputStream);
        signBitmap.recycle();
        resultBitmap.recycle();
        return new String(Base64.encode(byteArrayOutputStream.toByteArray(), Base64.NO_WRAP),
                StandardCharsets.US_ASCII);
    }
}
