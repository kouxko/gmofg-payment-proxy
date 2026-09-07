/*
 * ===========================================================================================
 * = COPYRIGHT
 *          PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION
 *   This software is supplied under the terms of a license agreement or nondisclosure
 *   agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or
 *   disclosed except in accordance with the terms in that agreement.
 *     Copyright (C) 2022-? PAX Computer Technology(Shenzhen) CO., LTD All rights reserved.
 * Revision History:
 * Date	                Author	                Action
 * 20231010 	        JerryJia               	Create
 * ===========================================================================================
 */
package jp.gmofg.cps.https.msg;

public class RefundData extends BaseData{

    private RefundResult result;

    @Override
    public RefundResult getBaseResult() {
        return result;
    }

    public void setResult(RefundResult refundResult) {
        this.result = refundResult;
    }


    public static class RefundResult extends BaseResult{

        private String currencyCode;
        private int refundAmount;

        public void setCurrencyCode(String currencyCode) {
            this.currencyCode = currencyCode;
        }
        public String getCurrencyCode() {
            return currencyCode;
        }

        public void setRefundAmount(int refundAmount) {
            this.refundAmount = refundAmount;
        }
        public int getRefundAmount() {
            return refundAmount;
        }
    }
}
